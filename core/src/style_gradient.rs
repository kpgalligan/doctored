//! The ONE gradient implementation the stroke's gradient fill and the
//! gradient overlay share: the box a fill spans (`gradient_box`), the
//! per-pixel parameter `t` with the box's trigonometry done once
//! (`GradientSampler`), and the colour and opacity at `t`
//! (`gradient_color`). Boxes and samples are in LAYER coordinates; the
//! plane model that maps them is `style_render`'s.
//!
//! Two options only a layer effect's gradient carries: a Shape Burst
//! stroke's or a gradient glow's `t` is READ from the effect's own
//! [`BurstField`] (`style_stroke_fill`, `style_glow_gradient`), which ignores
//! the box and the angle; and Dither
//! adds `(dissolve_threshold(⌊x⌋, ⌊y⌋) − 0.5) / dither_divisor` to `t` before
//! the clamp and Reverse — `adjust_map`'s rule, half a code of the depth the
//! ink is resolved at (255 on byte and 8-bit CMYK documents, 65535 at 16 and
//! 32 bits and on 16-bit CMYK; [`GradientSampler::with_dither_divisor`]).
//!
//! # Linear, Angle, Reflected and Radial, identified
//!
//! A Linear, Angle, Reflected or Radial fill reads the measured kernel,
//! ramp and geometry together ([`GradientSampler::color_with`],
//! [`GradientSampler::t`]): the Classic ramp (`gradient_field::classic_color`,
//! its module doc), the pixel's TOP-LEFT CORNER as the sample point, Linear's
//! extent `E` the length of the line through the box centre at the angle
//! inside the box, Reflected the Linear coordinate folded (`|2u / E|`, the
//! same `E`), and Angle's sweep CLOCKWISE on screen from the angle. Radial
//! measures from the INTEGER centre `(bx + ⌊bw/2⌋, by + ⌊bh/2⌋)` to the
//! pixel's integer coordinate (its top-left corner; on an even box that is
//! the corner from the box centre, on an odd one the pixel centre from the
//! box centre), over a radius that is the integer vector `(⌊R |cos a|⌋, ⌊R
//! |sin a|⌋)` of the line-in-box radius `R = E / 2`, its length the radius.
//! A Shape Burst stroke reads the Classic ramp too, of its own field's `t`
//! (`style_stroke_fill`, its module doc). Diamond and a glow's
//! field keep the linear ramp, and Diamond the geometry below, until theirs
//! is identified.
//!
//! Smoothness (`GradientFill::smoothness`): where the Classic ramp
//! applies, the colour is the linear and Classic ramps blended by the
//! fraction `k` (`gradient_field::smooth_color`, its module doc); at 1, the
//! default, the Classic path is taken untouched. Diamond and a glow's field
//! stay linear at any Smoothness; opacity stays linear everywhere. A
//! Gradient Fill LAYER holds the Classic term to each segment's bracketing
//! stops ([`fill_bound`], `gradient_field`'s "The bracket bound": the
//! confirm take's `f_n4_i100` rows, 1.0 / 0.010 codes where the unbounded
//! spline is 4.0 / 0.15), set by `artwork_render` and `cmyk_artwork_render`
//! through [`GradientSampler::with_classic_bound`]; the Gradient Overlay,
//! the Stroke's gradient and the glows keep it unbounded (unmeasured).
//!
//! Midpoints (`GradientStop::midpoint` and `opacity_midpoint`): between
//! stops `i` and `i + 1` the colour reads the segment fraction remapped
//! through stop `i`'s colour midpoint and the opacity through its opacity
//! midpoint (`gradient_field::midpoint_fraction`, its module doc), whatever
//! the style and ramp. At 50 % (the default) the fraction is untouched. A
//! glow's gradient and a legacy Gradient Map (no `method`) reset every
//! midpoint to 50 % when parsed (`style_json::glow_gradient`,
//! `adjust_map::GradientMap::parse`); a `method: classic` map keeps them and
//! takes the bracket-bounded ramp (`gradient_field::ClassicBound`).
//!
//! Why: `scripts/photoshop-probe/gradient_identify_673_fit.py
//! --judge-style-split` on one identification take (Photoshop 26.10.0, 254
//! cases): the 54 Linear identify rows meet 1.0 / 0.24 codes and the 52
//! Angle rows 1.0 / 0.46 outside the seam (today's 9.10 and 133.9 mean of
//! row means); every existing Linear and Angle row fell (fill-layers-672 GL8
//! 24 / 13.0 → 10 / 3.70, GA8 outside its seam 203 / 114.9 → 7 / 2.49, the
//! effects-578 Gradient Overlay 17.4 / 1.22 → 0.5 / 0.033, Stroke Gradient
//! `gl_o` 13.5 / 6.59 → 0.5 / 0.27), and every other style's rows stayed
//! byte-equal. Reflected by its `--judge-phase2` (ship rule (ii), the
//! controller's phase-2 ruling): 46 of its 48 identify rows meet 2 / 0.5
//! (the two misses 2.0 / 0.51 at 135° and scale 50, where today's was 16 /
//! 4.40), and every existing Reflected row fell (fill-layers-672 GF8 44 /
//! 20.55 → 20 / 5.84, style-parity-v1x b13_04 51.3 / 19.6 → 37.0 / 13.3,
//! b61_04 13.2 / 8.18 → 6.7 / 2.95). Radial by its `--judge-rint` on a
//! second take (402 cases, 216 Radial ones on six boxes, odd and even; ship
//! rule (ii), the controller's RULING-1 on take 2): every existing Radial
//! row fell (fill-layers-672 GR8 77 / 23.86 → 7 / 1.90, style-parity-v1x
//! b13_03 14.7 / 4.27 → 0.51 / 0.21, b61_03 171.9 / 85.5 → 0.51 / 0.053,
//! Stroke Gradient `gr_o` 17.97 / 10.27 → 0.53 / 0.23), all 48 take-1 rows
//! and the 132 take-2 rows outside its two named bins meet 2 / 0.5 (worst
//! 1.0 / 0.047); the misses are the bins, a radius component exactly on an
//! integer and odd boxes at scale 150. The Shape Burst by its
//! `--judge-phase3` on the same take (ship rule (ii)): every existing
//! Stroke Gradient Shape Burst row fell (`sb_c` 17.19 / 14.33 → 1.21 /
//! 0.50, `sb_o` 4.69 / 2.88 → 1.44 / 0.78), its numbers in
//! `style_stroke_fill`. Diamond's described extent (`trunc`, as Radial)
//! KEEPS: 108 of its 144 held-out rows meet 2 / 0.5, worst 8 / 2.26. The fit
//! runner's docstring holds every table.

use std::sync::Arc;

use crate::style::{GradientFill, GradientStyle};
use crate::style_render::{unit_color, RenderContext, Shape};

/// A per-pixel `t` read instead of the box: a Shape Burst stroke's
/// (`style_stroke_fill`) or a gradient glow's (`style_glow_gradient`) —
/// `t × 65535` over a `w × h` grid whose pixel (0, 0) sits at LAYER pixel
/// `origin`. Built once per effect and shared by its contributions (a Center
/// stroke's two halves read the same field); a pixel off the grid reads 0.
pub(crate) struct BurstField {
    pub origin: (i64, i64),
    pub w: u32,
    pub h: u32,
    pub t: Arc<[u16]>,
}

impl BurstField {
    /// `t` at layer pixel `(lx, ly)`, 0 off the grid.
    fn at(&self, lx: i64, ly: i64) -> f32 {
        let (x, y) = (lx - self.origin.0, ly - self.origin.1);
        if x < 0 || y < 0 || x >= i64::from(self.w) || y >= i64::from(self.h) {
            return 0.0;
        }
        self.t
            .get(y as usize * self.w as usize + x as usize)
            .map_or(0.0, |&v| f32::from(v) / 65535.0)
    }
}

/// The box a `GradientFill` spans, in LAYER coordinates `(x, y, w, h)`:
/// with `align_with_layer` the bounding box of the layer's non-transparent
/// pixels (`Shape::bounds` — module doc; the buffer rect when there are
/// none), so a Move drag keeps the cached plane; else the canvas rect,
/// `(-offset.x, -offset.y, canvas.w, canvas.h)`, so the gradient stays put
/// on the canvas while the layer moves. The ONE mapping the stroke's
/// gradient fill and the gradient overlay share.
pub(crate) fn gradient_box(
    fill: &GradientFill,
    shape: &Shape,
    ctx: &RenderContext,
) -> (f32, f32, f32, f32) {
    if fill.align_with_layer {
        match shape.bounds {
            Some((x0, y0, x1, y1)) => (x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32),
            None => (0.0, 0.0, shape.lw as f32, shape.lh as f32),
        }
    } else {
        (
            -(ctx.layer_offset.0 as f32),
            -(ctx.layer_offset.1 as f32),
            ctx.canvas.0 as f32,
            ctx.canvas.1 as f32,
        )
    }
}

/// A gradient fill over one box, with the per-box trigonometry done once:
/// `PlaneColor::Gradient` samples it per pixel on every composite, and a
/// sine and cosine per pixel would be most of that cost on a large layer.
#[derive(Clone)]
pub(crate) struct GradientSampler {
    pub fill: GradientFill,
    angle: f32,
    sin_a: f32,
    cos_a: f32,
    cx: f32,
    cy: f32,
    /// Linear's and Reflected's extent (module doc): `min(bw / |cos a|, bh /
    /// |sin a|) × scale`, a zero cosine or sine reading an infinite term.
    line_extent: f32,
    major: f32,
    /// Radial's integer centre (module doc): `(bx + ⌊bw/2⌋, by + ⌊bh/2⌋)`.
    rcx: f32,
    rcy: f32,
    /// Radial's extent (module doc, [`radial_extent`]), twice its radius.
    radial_extent: f32,
    /// A Shape Burst stroke's or a gradient glow's field
    /// ([`Self::with_burst`]); `None` otherwise.
    burst: Option<Arc<BurstField>>,
    /// `fill.dither` (module doc).
    dither: bool,
    /// Half a code of the depth the ink is resolved at: 255 from
    /// [`Self::new`], 65535 through [`Self::with_dither_divisor`].
    dither_divisor: f32,
    /// Where the Classic term runs (module doc, Smoothness): `Unbounded`
    /// from [`Self::new`], [`fill_bound`]'s through
    /// [`Self::with_classic_bound`] on a Gradient Fill layer only.
    bound: crate::gradient_field::ClassicBound,
}

impl GradientSampler {
    /// `fill` over the box `(bx, by, bw, bh)`.
    pub(crate) fn new(fill: &GradientFill, bx: f32, by: f32, bw: f32, bh: f32) -> Self {
        let angle = fill.angle.to_radians();
        let (sin_a, cos_a) = angle.sin_cos();
        let scale = if fill.scale.is_finite() {
            fill.scale.max(1e-6)
        } else {
            1.0
        };
        GradientSampler {
            fill: fill.clone(),
            angle,
            sin_a,
            cos_a,
            cx: bx + bw / 2.0,
            cy: by + bh / 2.0,
            line_extent: (bw / cos_a.abs()).min(bh / sin_a.abs()) * scale,
            major: bw.max(bh) * scale,
            rcx: bx + (bw / 2.0).floor(),
            rcy: by + (bh / 2.0).floor(),
            radial_extent: radial_extent(fill.angle, bw, bh, scale),
            burst: None,
            dither: fill.dither,
            dither_divisor: 255.0,
            bound: crate::gradient_field::ClassicBound::Unbounded,
        }
    }

    /// This sampler with its Classic term held by `bound` (module doc,
    /// Smoothness): the Gradient Fill layer's `artwork_render` and
    /// `cmyk_artwork_render` pass [`fill_bound`]'s, every other caller keeps
    /// [`Self::new`]'s `Unbounded`.
    pub(crate) fn with_classic_bound(mut self, bound: crate::gradient_field::ClassicBound) -> Self {
        self.bound = bound;
        self
    }

    /// This sampler reading an effect's `field` (module doc).
    pub(crate) fn with_burst(mut self, field: Arc<BurstField>) -> Self {
        self.burst = Some(field);
        self
    }

    /// The field this sampler reads, if any (the cache inventory
    /// records it, `resources_cache`).
    pub(crate) fn burst(&self) -> Option<&Arc<BurstField>> {
        self.burst.as_ref()
    }

    /// This sampler dithering by half a code of `divisor` (module doc): set
    /// where the ink is resolved, never stored in a cached plane.
    pub(crate) fn with_dither_divisor(mut self, divisor: f32) -> Self {
        self.dither_divisor = divisor;
        self
    }

    /// This sampler with every stop colour mapped through `f` — how an
    /// AUTHORED gradient reaches the document's colour space at composite
    /// time (`style_composite`). Only the stops change: the box, the angle
    /// and the trigonometry are geometry, so the clone is a stop list.
    ///
    /// Mapping the STOPS rather than each sampled colour means the ramp is
    /// interpolated in the document's space, which is the space everything
    /// else about the effect is composited in, and costs one conversion per
    /// stop instead of one per pixel.
    pub(crate) fn mapping_colors(&self, f: impl Fn([u8; 3]) -> [u8; 3]) -> Self {
        let mut out = self.clone();
        for stop in &mut out.fill.stops {
            stop.color = f(stop.color);
        }
        out
    }

    /// The gradient parameter `t` in [0, 1] at `(x, y)` (a pixel centre in
    /// the box's coordinates). With centre `c`, angle `a` and `u`/`v` the
    /// coordinates rotated by `a`: diamond `2 (|u| + |v|) / (max(bw, bh)
    /// scale)`. Linear, Reflected, Angle and Radial sample the pixel's
    /// top-left corner, `(x - 0.5, y - 0.5)` (module doc), `u` its projection:
    /// radial `2 |p - ci| / radial_extent` from the integer centre `ci`
    /// ([`radial_extent`]); linear `0.5 + u / line_extent`; reflected
    /// `|2u / line_extent|`; angle `((a - atan2(-(y - cy), x - cx)) mod 2pi)
    /// / 2pi`, clockwise on screen from `a`. So at angle 0 a linear gradient
    /// runs left to right, at 90 bottom to top (y-down).
    /// An attached field ([`Self::with_burst`]) is read at `(⌊x⌋, ⌊y⌋)`
    /// whatever the style; Shape Burst without one reads 0. Dither then adds its
    /// threshold (module doc). Clamped, then `1 - t` when `reverse`.
    pub(crate) fn t(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.cx;
        let dy = y - self.cy;
        let u = dx * self.cos_a - dy * self.sin_a;
        let v = dx * self.sin_a + dy * self.cos_a;
        // The top-left corner (module doc), for Linear, Reflected and Angle;
        // Radial takes it from its own centre.
        let (cdx, cdy) = (dx - 0.5, dy - 0.5);
        let cu = cdx * self.cos_a - cdy * self.sin_a;
        let t = match (&self.burst, self.fill.style) {
            // An attached field wins whatever the style (a glow's gradient
            // keeps the default Linear, `style_glow_gradient`).
            (Some(field), _) => field.at(x.floor() as i64, y.floor() as i64),
            (None, GradientStyle::Linear) => 0.5 + cu / self.line_extent,
            (None, GradientStyle::Reflected) => (2.0 * cu / self.line_extent).abs(),
            (None, GradientStyle::Radial) => {
                // The corner from the INTEGER centre (module doc).
                let (rx, ry) = (x - 0.5 - self.rcx, y - 0.5 - self.rcy);
                2.0 * (rx * rx + ry * ry).sqrt() / self.radial_extent
            }
            (None, GradientStyle::Diamond) => 2.0 * (u.abs() + v.abs()) / self.major,
            (None, GradientStyle::Angle) => {
                let tau = std::f32::consts::TAU;
                (self.angle - (-cdy).atan2(cdx)).rem_euclid(tau) / tau
            }
            (None, GradientStyle::ShapeBurst) => 0.0,
        };
        let t = if self.dither {
            let threshold = crate::blend::dissolve_threshold(x.floor() as i64, y.floor() as i64);
            t + (threshold - 0.5) / self.dither_divisor
        } else {
            t
        };
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.fill.reverse {
            1.0 - t
        } else {
            t
        }
    }
}

impl GradientSampler {
    /// Whether this sampler's colour follows the Classic ramp (module doc):
    /// with an attached field, a Shape Burst fill (a glow's field, style
    /// Linear, stays linear); without one, a Linear, Angle, Reflected or
    /// Radial fill. The field-less style list is `FieldStyle::classic`'s
    /// (gradient_field.rs): change the two together.
    fn classic(&self) -> bool {
        if self.burst.is_some() {
            return self.fill.style == GradientStyle::ShapeBurst;
        }
        matches!(
            self.fill.style,
            GradientStyle::Linear
                | GradientStyle::Angle
                | GradientStyle::Reflected
                | GradientStyle::Radial
        )
    }

    /// The straight colour and opacity of this sampler's fill at `t`, stop
    /// `i`'s colour read from `color_of(i)` (`N` components, as
    /// [`gradient_color_with`]): the Classic ramp's colour
    /// (`gradient_field::classic_color`) where the style is identified
    /// (module doc), blended toward the linear one by the fill's Smoothness
    /// (`gradient_field::smooth_color`), else [`gradient_color_with`]'s
    /// linear one at any Smoothness. The opacity
    /// is linear in both. Every renderer's colour of a box gradient goes
    /// through here, under the sampler's bound ([`Self::with_classic_bound`]:
    /// a Gradient Fill layer's [`fill_bound`], every other caller unbounded);
    /// a fill's opacity and a legacy Gradient Map read
    /// [`gradient_color_with`] directly, and a `method: classic` or
    /// `"perceptual"` map `gradient_field::smooth_color` with its bracket
    /// bound.
    pub(crate) fn color_with<const N: usize>(
        &self,
        t: f32,
        color_of: impl Fn(usize) -> [f32; N],
    ) -> ([f32; N], f32) {
        if !self.classic() {
            return gradient_color_with(&self.fill, t, color_of);
        }
        let stops = &self.fill.stops;
        // The opacity alone: no colour component is read.
        let (_, opacity) = gradient_color_with::<0>(&self.fill, t, |_| []);
        let color = crate::gradient_field::smooth_color(
            stops.len(),
            t,
            self.fill.smoothness,
            self.bound,
            |i| stops[i].position,
            |i| stops[i].midpoint,
            color_of,
        );
        (color, opacity)
    }
}

/// The Classic bound a Gradient Fill LAYER of `style` takes (module doc,
/// Smoothness; `gradient_field`'s "The bracket bound"), the ONE home of the
/// choice: `Bracket` for Linear, Radial, Angle and Reflected, `Unbounded` for
/// Diamond (whose ramp stays linear) and the Shape Burst (a stroke's only).
pub(crate) fn fill_bound(style: GradientStyle) -> crate::gradient_field::ClassicBound {
    use crate::gradient_field::ClassicBound;
    match style {
        GradientStyle::Linear
        | GradientStyle::Radial
        | GradientStyle::Angle
        | GradientStyle::Reflected => ClassicBound::Bracket,
        GradientStyle::Diamond | GradientStyle::ShapeBurst => ClassicBound::Unbounded,
    }
}

/// Radial's extent `E` over a `bw × bh` box at `angle_degrees` and `scale`
/// (module doc): `2 hypot(trunc(R |cos a|), trunc(R |sin a|))`, `R = min(bw /
/// |cos a|, bh / |sin a|) · scale / 2`, a term whose cosine or sine is within
/// 1e-12 of zero dropped. Computed in f64, as the fit runner's `trunc_extent`
/// judged it: the truncation is what moves a component that lands near an
/// integer, so the box arithmetic must not round first. A box too small to
/// hold one whole pixel of radius (`E` 0) reads 1e-6, so every pixel but the
/// centre (at least one pixel from it) is the end colour, not a NaN or an
/// infinity, which `t` reads as 0.
fn radial_extent(angle_degrees: f32, bw: f32, bh: f32, scale: f32) -> f32 {
    let a = f64::from(angle_degrees).to_radians();
    let (ca, sa) = (a.cos().abs(), a.sin().abs());
    let line = [(f64::from(bw), ca), (f64::from(bh), sa)]
        .into_iter()
        .filter(|&(_, c)| c > 1e-12)
        .map(|(b, c)| b / c)
        .fold(f64::INFINITY, f64::min)
        * f64::from(scale);
    let r = line / 2.0;
    let e = 2.0 * (r * ca).trunc().hypot((r * sa).trunc());
    if e.is_finite() && e > 0.0 {
        e as f32
    } else {
        1e-6
    }
}

/// The straight RGB and opacity of `fill` at `t`: linear between the two
/// bracketing stops (stops are sorted by position; of two at the same
/// position the later one wins past it) at the fraction remapped through the
/// left stop's colour and opacity midpoints (module doc), clamped to the end
/// stops.
pub(crate) fn gradient_color(fill: &GradientFill, t: f32) -> ([f32; 3], f32) {
    gradient_color_with(fill, t, |i| unit_color(fill.stops[i].color))
}

/// [`gradient_color`] with stop `i`'s colour read from `color_of(i)` — the ONE
/// interpolation, which a 32-bit float document runs over its stops already
/// carried into LINEAR light (`style_float`: measured against
/// Photoshop, a 32-bit gradient interpolates between linear stops) and a
/// CMYK document over its stops' INK, four components (#547,
/// `cmyk_style_row`; take1's G-INK). Generic over the component count `N`;
/// `N = 3` is the RGB arithmetic statement for statement. `color_of` is
/// asked only for indices of `fill.stops`.
pub(crate) fn gradient_color_with<const N: usize>(
    fill: &GradientFill,
    t: f32,
    color_of: impl Fn(usize) -> [f32; N],
) -> ([f32; N], f32) {
    let stops = &fill.stops;
    // The bracket is `gradient_field::segment`'s, the one search of a stops
    // list; `(i, i, _)` is an end stop or NaN, returned untouched.
    let Some((i, j, f)) = crate::gradient_field::segment(stops.len(), t, |i| stops[i].position)
    else {
        return ([0.0; N], 0.0);
    };
    let at = |i: usize| (color_of(i), stops[i].opacity);
    if i == j {
        return at(i);
    }
    let (ca, oa) = at(i);
    let (cb, ob) = at(j);
    let fc = crate::gradient_field::midpoint_fraction(f, stops[i].midpoint);
    let fo = crate::gradient_field::midpoint_fraction(f, stops[i].opacity_midpoint);
    (
        std::array::from_fn(|c| ca[c] + (cb[c] - ca[c]) * fc),
        oa + (ob - oa) * fo,
    )
}
