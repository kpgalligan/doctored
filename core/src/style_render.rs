//! The layer-style plane renderer: the padded shape plane every effect is a
//! function of, the plane primitives (ONE blur, ONE dilate/erode, shift,
//! multiply) and the per-effect dispatch. The gradient math is
//! `style_gradient`; the rendered-plane cache that lives inside each
//! `Arc<LayerStyle>` is `style_cache`.
//!
//! # The plane model
//!
//! Effects are functions of the layer's SHAPE — its straight alpha times its
//! enabled mask, `coverage = round(alpha * mask / 255)` (the arithmetic of
//! `remove_mask(apply: true)`) — on a plane padded by `pad` on every side.
//! A [`Shape`] carries the plane's size, its coverage bytes and its
//! `origin`: the LAYER coordinate of plane pixel `(0, 0)`, so plane pixel
//! `p` is layer pixel `p + origin` and canvas pixel `offset + p + origin`.
//! A full plane has `origin = (-pad, -pad)` and size `lw + 2 pad` by
//! `lh + 2 pad`, with 0 coverage in the padding. Planes are LAYER-local, so
//! a Move drag, an opacity scrub and Duplicate Layer all reuse them. `pad`
//! is `LayerStyle::pad()`: each enabled effect's reach plus one, so a plane
//! never reads or writes beyond its border and a shape padded by `reach`
//! renders exactly as it would on an infinite plane. A plane over
//! `MAX_PIXELS` is refused (`Shape::of_layer` is `None`) and the style
//! renders no effects — a documented refusal, never a panic.
//!
//! The same locality is what makes a SUB-PLANE exact: [`Shape::sub`] cuts a
//! window out of a full plane (its `origin` moves with it, its `bounds`
//! stay the full shape's), and every effect rendered over the sub-plane
//! equals the full render wherever the window's border is at least the
//! effect's reach away — the cache's incremental re-render for a brush
//! stroke (`style_cache`) relies on it, so no renderer may read anything
//! global about the plane beyond `bounds` and `lw`/`lh`.
//!
//! `bounds` is the bounding box of the layer's non-transparent pixels
//! (alpha > 0), in layer coordinates: what "Align with Layer" spans a
//! gradient across. Transparent pixels are ignored for that box, and
//! a new, shape-tool or brush-painted layer is a canvas-sized buffer whose
//! content is much smaller — the buffer rect would spread the gradient
//! over the whole canvas. The enabled mask, though part of the shape, is
//! left out of the box on purpose: masking away part of a gradient-filled
//! shape must not make the gradient jump to re-span what remains.
//!
//! A [`Contribution`] is what one effect emits: a coverage plane the size of
//! the shape, a colour, a blend mode, an opacity, a placement (Below the
//! pixels or Interior, above them), an integer shift applied at composite
//! time, a knock-out flag, and which [`Part`] of its effect it is (the
//! bevel's highlight or shadow; `Main` for every other effect) — the tag
//! `style_reuse` re-stamps by. The colour is one solid RGB or a
//! [`PlaneColor::Gradient`] evaluated per pixel AT COMPOSITE TIME from the
//! pixel's layer coordinate — a gradient is a closed-form function of
//! position and a few stops, and storing it per pixel would pin 12 bytes
//! per plane pixel in every cache entry (a 20 MP layer, a quarter gigabyte
//! per entry). Every Interior contribution's coverage is bounded by the
//! shape (its renderer multiplies by `shape.coverage`), which is what lets
//! `style_composite` draw it straight onto the accumulator.
//!
//! The numeric conventions and the plane primitives live in
//! `style_plane_ops` (Phase 13A-5b-i), re-exported here so `style_render::`
//! stays their one path.
//!
//! the padded shape plane (with sub-planes) and the per-effect dispatch (`render_effects_with_job`
//! / `render_effect_with_job` poll a `JobContext`).
//!
//! `style_fx_*.rs` / `style_blend_if.rs` — one effect each (drop shadow, inner shadow, outer glow,
//! inner glow, stroke, color overlay, gradient overlay, bevel/emboss, satin) and the Blend If
//! weight; each is a pure function of the shape and must stay local to its reach (the sub-plane
//! re-render relies on it).

use crate::authored_color::{srgb_numbers, wide_into, AuthoredColors};
use crate::blend::{blend_kind, BlendKind, BlendMode};
use crate::doc::{Layer, MAX_PIXELS};
use crate::job_control::JobContext;
use crate::lab_blend::LabGrid;
use crate::pattern_tile::PatternSampler;
use crate::style::{Effect, EffectKind, GlobalLight, LayerStyle};
use crate::style_float::FloatStyle;
use crate::style_gradient::GradientSampler;
pub(crate) use crate::style_plane_ops::{
    blur_plane_with_job, blur_reach, dilate_plane_with_job, downsample_factor,
    erode_plane_with_job, feather_radius, inside_stroke_band, invert_plane, light_offset,
    multiply_planes, reduced_sigma, shift_plane, sigma_for_size, spread_split, SUB_PLANE_ALIGN,
};
use crate::{
    style_fx_bevel_emboss, style_fx_color_overlay, style_fx_drop_shadow, style_fx_gradient_overlay,
    style_fx_inner_glow, style_fx_inner_shadow, style_fx_outer_glow, style_fx_pattern_overlay,
    style_fx_satin, style_fx_stroke,
};

// ------------------------------------------------------------------ shape --

/// The padded coverage plane an effect stack renders from (module doc).
#[derive(Clone)]
pub(crate) struct Shape {
    pub lw: u32,
    pub lh: u32,
    pub w: u32,
    pub h: u32,
    /// Layer coordinate of plane pixel (0, 0): `(-pad, -pad)` for a full
    /// plane, further in for a sub-plane.
    pub origin: (i64, i64),
    pub coverage: Vec<u8>,
    /// Bounding box `(x0, y0, x1, y1)` (exclusive) of the FULL layer's
    /// non-transparent pixels in layer coordinates, the mask left out
    /// (module doc); `None` when there are none.
    pub bounds: Option<(i64, i64, i64, i64)>,
}

impl Shape {
    /// The shape of `layer` on a plane padded by `pad`; `Ok(None)` when the
    /// plane would exceed `MAX_PIXELS`. Polls `job` at the head of every row
    /// (Phase 13A-5b-ii-b).
    pub(crate) fn of_layer(
        layer: &Layer,
        pad: u32,
        job: JobContext<'_>,
    ) -> Result<Option<Shape>, String> {
        let (lw, lh) = layer.pixels.dimensions();
        let w = u64::from(lw) + 2 * u64::from(pad);
        let h = u64::from(lh) + 2 * u64::from(pad);
        if w * h > MAX_PIXELS || w > u64::from(u32::MAX) || h > u64::from(u32::MAX) {
            return Ok(None);
        }
        let (w, h) = (w as u32, h as u32);
        let mut coverage = vec![0u8; w as usize * h as usize];
        let active_mask = layer.active_mask();
        let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
        let (mut bx0, mut by0, mut bx1, mut by1) = (i64::from(lw), i64::from(lh), 0i64, 0i64);
        for y in 0..lh {
            job.check()?;
            let row = (y + pad) as usize * w as usize + pad as usize;
            let mut row_min = lw;
            let mut row_max = 0;
            for (sx, run) in layer.pixels.row_segments(y, 0, lw) {
                for (i, px) in run.iter().enumerate() {
                    let x = sx + i as u32;
                    let alpha = px[3];
                    let c = Self::coverage_byte(
                        alpha,
                        mask.map(|m| m[y as usize * lw as usize + x as usize]),
                    );
                    coverage[row + x as usize] = c;
                    if alpha != 0 {
                        row_min = row_min.min(x);
                        row_max = x;
                    }
                }
            }
            if row_min < lw {
                bx0 = bx0.min(i64::from(row_min));
                bx1 = bx1.max(i64::from(row_max) + 1);
                by0 = by0.min(i64::from(y));
                by1 = i64::from(y) + 1;
            }
        }
        let bounds = (bx1 > bx0).then_some((bx0, by0, bx1, by1));
        Ok(Some(Shape {
            lw,
            lh,
            w,
            h,
            origin: (-i64::from(pad), -i64::from(pad)),
            coverage,
            bounds,
        }))
    }

    /// `of_layer`'s pixel: `alpha`, or `round(alpha·mask/255)` with a mask.
    /// The regional compositor (`doc_regional`) reads a styled clip base's
    /// shape sample by sample through this same statement.
    #[inline]
    pub(crate) fn coverage_byte(alpha: u8, mask: Option<u8>) -> u8 {
        match mask {
            None => alpha,
            Some(m) => (f32::from(alpha) * f32::from(m) / 255.0).round() as u8,
        }
    }

    /// Row-major index of plane pixel (x, y).
    pub(crate) fn idx(&self, x: u32, y: u32) -> usize {
        y as usize * self.w as usize + x as usize
    }

    /// Coverage at plane pixel (x, y); 0 outside the plane.
    pub(crate) fn at(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= i64::from(self.w) || y >= i64::from(self.h) {
            return 0;
        }
        self.coverage[self.idx(x as u32, y as u32)]
    }

    /// `255 - coverage`: the layer's transparency. Also the ONE knock-out
    /// of every exterior effect (outer glow, the outside stroke band): a
    /// plane multiplied by it holds at most `1 - alpha` of the effect under
    /// a partial pixel — the same proportion the drop shadow's
    /// composite-time knock-out (`knocked_out_by_shape`) leaves — so a
    /// semi-transparent layer never has its interior filled with glow or
    /// stroke colour, and an anti-aliased edge fades the effect out
    /// smoothly rather than at a hard threshold.
    pub(crate) fn inverted(&self) -> Vec<u8> {
        self.coverage.iter().map(|&v| 255 - v).collect()
    }

    /// The layer rect in plane coordinates, `(x0, y0, x1, y1)` exclusive,
    /// clipped to the plane: the full layer rect on a full plane, the
    /// window's part of it on a sub-plane.
    pub(crate) fn layer_rect(&self) -> (u32, u32, u32, u32) {
        let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi)) as u32;
        (
            clamp(-self.origin.0, self.w),
            clamp(-self.origin.1, self.h),
            clamp(i64::from(self.lw) - self.origin.0, self.w),
            clamp(i64::from(self.lh) - self.origin.1, self.h),
        )
    }

    /// The window `(x0, y0, x1, y1)` (plane coordinates, exclusive, inside
    /// the plane) of this shape as a shape of its own: the coverage bytes
    /// of the window, the origin moved by its corner, `bounds` and the
    /// layer size unchanged (module doc).
    pub(crate) fn sub(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> Shape {
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut coverage = Vec::with_capacity(w as usize * h as usize);
        for y in y0..y0 + h {
            let row = self.idx(x0, y);
            coverage.extend_from_slice(&self.coverage[row..row + w as usize]);
        }
        Shape {
            lw: self.lw,
            lh: self.lh,
            w,
            h,
            origin: (self.origin.0 + i64::from(x0), self.origin.1 + i64::from(y0)),
            coverage,
            bounds: self.bounds,
        }
    }

    /// The bounding box `(x0, y0, x1, y1)` (plane coordinates, exclusive)
    /// of the pixels whose coverage differs between two planes of the same
    /// size; `None` when they are identical (or differently sized).
    pub(crate) fn diff_bounds(&self, other: &Shape) -> Option<(u32, u32, u32, u32)> {
        if self.w != other.w || self.h != other.h || self.coverage.len() != other.coverage.len() {
            return None;
        }
        let w = self.w as usize;
        let (mut x0, mut y0, mut x1, mut y1) = (w, self.h as usize, 0usize, 0usize);
        for y in 0..self.h as usize {
            let a = &self.coverage[y * w..(y + 1) * w];
            let b = &other.coverage[y * w..(y + 1) * w];
            if a == b {
                continue;
            }
            let first = a.iter().zip(b).position(|(p, q)| p != q).unwrap_or(0);
            let last = a.iter().zip(b).rposition(|(p, q)| p != q).unwrap_or(w - 1);
            x0 = x0.min(first);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y + 1;
        }
        (x1 > x0).then_some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
    }
}

// ---------------------------------------------------------- contributions --

/// Straight RGB in 0..1: one colour for the whole plane, or a gradient
/// evaluated per pixel at composite time (module doc), sampled at the
/// pixel's centre in LAYER coordinates. `LinearGradient` is a 32-bit float
/// document's gradient (`style_float`): the same sampler with
/// its stops already in the document's LINEAR light, one per `fill.stops`
/// entry, interpolated between there — never cached, since only a
/// composite-time ink holds one. `Pattern` is a pattern fill's placed
/// tile level (`pattern_tile`; a pattern overlay's or a Pattern stroke's),
/// sampled at the pixel's corner-aligned grid point: its samples are DOCUMENT values at every depth (take1's FLOAT =
/// VERBATIM), so no conversion ever touches it — except on a Lab layer,
/// whose texels are authored sRGB and became Lab codes when the level was
/// built ([`RenderContext::lab_tiles`], `lab_style::painted_sampler`). `Wide`
/// is a solid colour
/// authored beyond the sRGB byte grid: its LINEAR-LIGHT sRGB-primaries
/// components, extended range, which [`PlaneColor::resolved`] turns into the
/// document's numbers at composite time (`authored_color::wide_into`).
#[derive(Clone)]
pub(crate) enum PlaneColor {
    Solid([f32; 3]),
    Wide([f32; 3]),
    Gradient(GradientSampler),
    LinearGradient(GradientSampler, Vec<[f32; 3]>),
    Pattern(PatternSampler),
}

impl PlaneColor {
    /// This colour converted through `t` — the AUTHORED sRGB colour a style
    /// stores turned into the document's own numbers, once, at composite
    /// time (`style_composite::composite_contribution`, the one place a
    /// contribution's colour is read).
    ///
    /// Deliberately NOT applied when the plane is rendered: the cache holds
    /// the colours the user authored, so an Assign or Convert to Profile
    /// leaves every cached plane valid and a style's APPEARANCE survives a
    /// profile change without the style JSON being rewritten.
    ///
    /// A `Solid` came from [`unit_color`], so `* 255` recovers the authored
    /// byte exactly.
    pub(crate) fn converted(&self, t: &AuthoredColors) -> PlaneColor {
        match self {
            PlaneColor::Solid(rgb) => {
                let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                PlaneColor::Solid(unit_color(t.color([
                    byte(rgb[0]),
                    byte(rgb[1]),
                    byte(rgb[2]),
                ])))
            }
            PlaneColor::Wide(linear) => PlaneColor::Solid(t.wide(*linear, false)),
            PlaneColor::Gradient(sampler) => {
                PlaneColor::Gradient(sampler.mapping_colors(|c| t.color(c)))
            }
            // Already the float document's own linear numbers: an encoded
            // conversion never meets one (`Contribution::ink`).
            PlaneColor::LinearGradient(..) => self.clone(),
            // The tile's samples are the document's own values.
            PlaneColor::Pattern(_) => self.clone(),
        }
    }

    /// This colour in the document's numbers under `t` (`None`: sRGB, or a
    /// profile that keeps the authored numbers): a `Wide` colour through
    /// [`wide_into`] — `deep` a 16-bit scene's fractional numbers, else the
    /// byte grid's — and every other colour as [`PlaneColor::converted`]
    /// (unchanged when `t` is `None`).
    pub(crate) fn resolved(&self, t: Option<&AuthoredColors>, deep: bool) -> PlaneColor {
        match (self, t) {
            (PlaneColor::Wide(linear), t) => PlaneColor::Solid(wide_into(t, *linear, deep)),
            (_, Some(t)) => self.converted(t),
            (_, None) => self.clone(),
        }
    }

    /// The colour at layer pixel `(lx, ly)`. A `Wide` colour is resolved
    /// before a composite reads it (`Contribution::ink`); read unresolved it
    /// is its sRGB numbers.
    pub(crate) fn at(&self, lx: i64, ly: i64) -> [f32; 3] {
        match self {
            PlaneColor::Solid(rgb) => *rgb,
            PlaneColor::Wide(linear) => srgb_numbers(*linear),
            PlaneColor::Gradient(sampler) => {
                let t = sampler.t(lx as f32 + 0.5, ly as f32 + 0.5);
                sampler
                    .color_with(t, |i| unit_color(sampler.fill.stops[i].color))
                    .0
            }
            PlaneColor::LinearGradient(sampler, stops) => {
                let t = sampler.t(lx as f32 + 0.5, ly as f32 + 0.5);
                sampler.color_with(t, |i| stops[i]).0
            }
            PlaneColor::Pattern(sampler) => {
                let p = sampler.at(lx, ly);
                [p[0], p[1], p[2]]
            }
        }
    }
}

/// How a composite turns a contribution's AUTHORED colour and blend into what
/// it draws: `Encoded` is every byte scene — the authored sRGB
/// colour through the document's byte conversion (`None` for sRGB or a LUT
/// profile that cannot convert both ways) and `blend::blend_kind` —, `Encoded16`
/// a u16 scene's, the same statements with a gradient's Dither at half a
/// 16-bit code (`style_gradient`) and, in a Lab Color document, its
/// grid (the kind is `lab_blend`'s), and `Float` a 32-bit float
/// document's (`style_float`): the colour in the document's LINEAR light and
/// the measured float kind. A borrow, so composite environments stay `Copy`.
#[derive(Clone, Copy)]
pub(crate) enum StyleColors<'a> {
    Encoded(Option<&'a AuthoredColors>),
    Encoded16(Option<&'a AuthoredColors>, Option<LabGrid>),
    Float(&'a FloatStyle),
}

/// What one contribution draws with: its colour in the document's numbers
/// and its blend behaviour, resolved once per contribution (never per pixel).
#[derive(Clone)]
pub(crate) struct Ink {
    pub color: PlaneColor,
    pub kind: BlendKind,
}

/// Where a contribution composites relative to the layer's pixels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    Below,
    Interior,
}

/// Which of its effect's parts a contribution is: `Main` for every effect
/// that colours all of its planes alike, `Highlight` / `Shadow` for the
/// bevel's two lit sides, which carry their own colour, blend and opacity.
/// What tells them apart when a cached plane is re-stamped with a new
/// style's colours (`style_reuse`) — the planes alone cannot (a bevel
/// whose highlight faces nowhere emits its shadow only).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Main,
    Highlight,
    Shadow,
}

/// What one effect emits (module doc).
#[derive(Clone)]
pub(crate) struct Contribution {
    pub kind: EffectKind,
    pub part: Part,
    pub coverage: Vec<u8>,
    pub color: PlaneColor,
    pub blend: BlendMode,
    pub opacity: f32,
    pub placement: Placement,
    /// Integer canvas shift applied at composite time (Below only).
    pub shift: (i32, i32),
    /// Multiply the coverage by `1 - shape` at the landing position
    /// (the "Layer Knocks Out Drop Shadow" option).
    pub knocked_out_by_shape: bool,
    /// The effect's ordinal among the ENABLED instances of its kind, in
    /// render order (`style_instances`): 0 for the lowest. Each
    /// renderer emits 0; [`render_effects_with_job`] stamps the real one.
    pub instance: u8,
}

impl Contribution {
    /// A solid-colour, unshifted, non-knocked-out `Main` contribution;
    /// `color` is [`slot_color`]'s.
    pub(crate) fn solid(
        kind: EffectKind,
        coverage: Vec<u8>,
        color: PlaneColor,
        blend: BlendMode,
        opacity: f32,
        placement: Placement,
    ) -> Self {
        Contribution {
            kind,
            part: Part::Main,
            coverage,
            color,
            blend,
            opacity,
            placement,
            shift: (0, 0),
            knocked_out_by_shape: false,
            instance: 0,
        }
    }

    /// This contribution's [`Ink`] under `colors`: `Encoded` converts the
    /// authored colour through the transform (`PlaneColor::converted`) and
    /// takes `blend_kind(self.blend)` — exactly the statements every byte and
    /// 16-bit composite ran before the 32-bit float styles — `Encoded16` the same with a
    /// gradient's Dither divisor 65535 and, in a Lab Color document,
    /// the Lab arm (`lab_blend::kind_in`: every effect's colour blend
    /// in a Lab document comes from here), and `Float` asks the
    /// [`FloatStyle`].
    pub(crate) fn ink(&self, colors: StyleColors<'_>) -> Ink {
        match colors {
            StyleColors::Encoded(transform) => Ink {
                color: self.color.resolved(transform, false),
                kind: blend_kind(self.blend),
            },
            StyleColors::Encoded16(transform, lab) => Ink {
                color: match self.color.resolved(transform, true) {
                    PlaneColor::Gradient(sampler) => {
                        PlaneColor::Gradient(sampler.with_dither_divisor(65535.0))
                    }
                    other => other,
                },
                kind: crate::lab_blend::kind_in(self.blend, false, lab),
            },
            StyleColors::Float(float) => Ink {
                color: float.plane_color(&self.color),
                kind: float.effect_kind(self.blend),
            },
        }
    }

    /// Whether two contributions are the same slot of the same style — the
    /// per-pixel planes aside: the same effect instance, part, placement,
    /// shift and knock-out (blend, opacity and colour follow from the style).
    pub(crate) fn same_slot(&self, other: &Contribution) -> bool {
        self.kind == other.kind
            && self.instance == other.instance
            && self.part == other.part
            && self.placement == other.placement
            && self.shift == other.shift
            && self.knocked_out_by_shape == other.knocked_out_by_shape
    }
}

/// One solid colour slot's plane colour: its `linear` as
/// [`PlaneColor::Wide`] when the slot holds one, else its sRGB byte.
pub(crate) fn slot_color(color: [u8; 3], linear: Option<[f32; 3]>) -> PlaneColor {
    match linear {
        Some(linear) => PlaneColor::Wide(linear),
        None => PlaneColor::Solid(unit_color(color)),
    }
}

/// `[u8; 3]` to straight 0..1.
pub(crate) fn unit_color(color: [u8; 3]) -> [f32; 3] {
    [
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
    ]
}

/// What a render depends on beyond the shape and the style: the global
/// light, and — for canvas-aligned gradients — the DOCUMENT canvas size and
/// the layer's offset on it (never the projection window: Merge Down
/// composites into the union of two layers, and a canvas-aligned gradient
/// must bake exactly as the projection showed it). `lab_tiles` is true on a
/// Lab document only (`authored_color::AuthoredColors::converts_tiles`): its
/// pattern texels convert from sRGB to Lab codes (`lab_style::painted_sampler`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderContext {
    pub light: GlobalLight,
    pub canvas: (u32, u32),
    pub layer_offset: (i32, i32),
    pub lab_tiles: bool,
}

/// The rendered planes of one style over one shape; both lists sorted by
/// render rank.
pub(crate) struct RenderedEffects {
    pub shape: Shape,
    pub below: Vec<Contribution>,
    pub interior: Vec<Contribution>,
}

impl RenderedEffects {
    /// Indices in `interior` of the inside bands (an Inside Stroke, or a
    /// Center Stroke's inner half: every Stroke contribution in `interior`),
    /// ascending — render order; empty without one. Each is a contribution
    /// `style_composite::replace_under_band` composites over the stack
    /// BENEATH the layer rather than over its pixels, and with stacked
    /// strokes each band replaces in turn over the same B0 (take1's
    /// rule R).
    pub(crate) fn inside_bands(&self) -> Vec<usize> {
        self.interior
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == EffectKind::Stroke)
            .map(|(i, _)| i)
            .collect()
    }
}

/// Render rank within a placement (the dialog order read bottom-up;
/// interior effects use their own blend modes — "Blend Interior Effects as
/// Group" is off).
fn rank(kind: EffectKind, placement: Placement) -> u8 {
    match placement {
        Placement::Below => match kind {
            EffectKind::DropShadow => 0,
            EffectKind::OuterGlow => 1,
            EffectKind::Stroke => 2,
            EffectKind::BevelEmboss => 3,
            _ => 4,
        },
        Placement::Interior => match kind {
            EffectKind::PatternOverlay => 0,
            EffectKind::GradientOverlay => 1,
            EffectKind::ColorOverlay => 2,
            EffectKind::Satin => 3,
            EffectKind::InnerGlow => 4,
            EffectKind::InnerShadow => 5,
            EffectKind::Stroke => 6,
            EffectKind::BevelEmboss => 7,
            _ => 8,
        },
    }
}

/// Renders every enabled effect of `style` over `shape` and partitions the
/// contributions by placement in render order, polling `job` before the
/// loop, inside every effect's renderer and after the ranking (Phase
/// 13A-5b-i). Effect `i` of the `n` enabled ones renders under
/// `job.range(i/n, (i + 1)/n)`, an
/// EQUAL split because `style_scratch` charges a uniform `8·p` per enabled
/// effect (Phase 13A-5b-iii-c-1): its first blur ramps its own sub-range and
/// its later blurs hold that sub-range's end. Under a `checkpoints_only` job
/// every sub-range is zero-width, so nothing reports.
pub(crate) fn render_effects_with_job(
    style: &LayerStyle,
    shape: Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<RenderedEffects, String> {
    job.check()?;
    let mut all = Vec::new();
    let n = style.enabled_effects().count();
    let mut ordinals = Ordinals::default();
    for (i, effect) in style.enabled_effects().enumerate() {
        let part = job.range(i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let instance = ordinals.next(effect.kind());
        all.extend(
            render_effect_with_job(effect, &shape, ctx, part)?
                .into_iter()
                .map(|c| Contribution { instance, ..c }),
        );
    }
    let (below, interior) = ranked(all);
    job.check()?;
    Ok(RenderedEffects {
        shape,
        below,
        interior,
    })
}

/// Counts the enabled instances of each kind as a walk meets them in render
/// order, so each effect learns its [`Contribution::instance`].
#[derive(Default)]
pub(crate) struct Ordinals([u8; EffectKind::ALL.len()]);

impl Ordinals {
    /// The ordinal of the next effect of `kind` (0 for its first).
    pub(crate) fn next(&mut self, kind: EffectKind) -> u8 {
        let slot = &mut self.0[kind as usize];
        let ordinal = *slot;
        *slot = slot.saturating_add(1);
        ordinal
    }
}

/// The one dispatch to an effect's renderer: `effect`'s contributions
/// over `shape` (any number, any placement, unsorted), polling `job`
/// (13A-5b-i).
pub(crate) fn render_effect_with_job(
    effect: &Effect,
    shape: &Shape,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    match effect {
        Effect::DropShadow(fx) => style_fx_drop_shadow::render(fx, shape, ctx, job),
        Effect::InnerShadow(fx) => style_fx_inner_shadow::render(fx, shape, ctx, job),
        Effect::OuterGlow(fx) => style_fx_outer_glow::render(fx, shape, ctx, job),
        Effect::InnerGlow(fx) => style_fx_inner_glow::render(fx, shape, ctx, job),
        Effect::Stroke(fx) => style_fx_stroke::render(fx, shape, ctx, job),
        Effect::ColorOverlay(fx) => style_fx_color_overlay::render(fx, shape, ctx, job),
        Effect::GradientOverlay(fx) => style_fx_gradient_overlay::render(fx, shape, ctx, job),
        Effect::PatternOverlay(fx) => style_fx_pattern_overlay::render(fx, shape, ctx, job),
        Effect::BevelEmboss(fx) => style_fx_bevel_emboss::render(fx, shape, ctx, job),
        Effect::Satin(fx) => style_fx_satin::render(fx, shape, ctx, job),
    }
}

/// Partitions contributions by placement, each list in render order
/// (a stable sort, so an effect's own emission order survives within its
/// rank).
pub(crate) fn ranked(all: Vec<Contribution>) -> (Vec<Contribution>, Vec<Contribution>) {
    let (mut below, mut interior): (Vec<Contribution>, Vec<Contribution>) = all
        .into_iter()
        .partition(|c| c.placement == Placement::Below);
    below.sort_by_key(|c| rank(c.kind, Placement::Below));
    interior.sort_by_key(|c| rank(c.kind, Placement::Interior));
    (below, interior)
}
