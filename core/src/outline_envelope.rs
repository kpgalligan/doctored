//! WARP TEXT'S ENVELOPE — the point map that bends a text block's box onto
//! itself (designs/warp-text.md, WT-2), and the box its image covers (WT-4).
//! A kernel: `outline_map::OutlineMap::Envelope` sends glyph outlines and
//! decorations through `Envelope::point`, and `dr_outline_map_bounds` reads
//! `Envelope::bounds`. It names no document type.
//!
//! **The formulas.** The box is `[x0, y0, w, h]`, centre `(xc, yc)`. A point
//! `(x, y)` has fractions `u = (x − x0)/w`, `v = (y − y0)/h` across and down
//! it, `c = 2u − 1`, `d = 2v − 1`, `p(t) = 1 − t²`, and `b` is the bend:
//! - `arch`: `y′ = y − b·(h/2)·p(c)`
//! - `upper`: `y′ = y − b·(h/2)·p(c)·(1 − v)`
//! - `lower`: `y′ = y + b·(h/2)·p(c)·v`
//! - `bulge`: `y′ = yc + (y − yc)·(1 + 0.75·b·p(c))`
//! - `squeeze`: `x′ = xc + (x − xc)·(1 − 0.75·b·p(d))`
//! - `inflate`: `x′ = xc + (x − xc)·(1 + 0.5·b·p(d))`,
//!   `y′ = yc + (y − yc)·(1 + 0.5·b·p(c))`
//! - `flag`: `y′ = y + b·(h/2)·sin(2πu)`
//! - `rise`: `y′ = y − b·(h/2)·sin(π(u − ½))`
//! - `arc`: the box wrapped on a circle of radius `R = w/(π·|b|)` at angle
//!   `θ = π·|b|·(u − ½)`. For `b > 0` the centre is `C = (xc, y0 + h + R)`, a
//!   point lies at radius `r = R + h·(1 − v)` and maps to
//!   `(Cx + r·sin θ, Cy − r·cos θ)`; for `b < 0`, `C = (xc, y0 − R)`,
//!   `r = R + h·v` and the point maps to `(Cx + r·sin θ, Cy + r·cos θ)`. A
//!   bend of ±1 is a half circle and the text keeps its left-to-right order
//!   for either sign; `|b| < 1e-6` is the identity.
//!
//! Then the two distortions, the same for every style: `horizontal` scales
//! height across the box, `y″ = yc + (y′ − yc)·(1 + 0.75·horizontal·c)`, and
//! `vertical` scales width down it, `x″ = xc + (x′ − xc)·(1 + 0.75·vertical·d)`.
//! The 0.75 and 0.5 factors keep every scale in `[0.25, 1.75]`, so no scaling
//! style folds the box.
//!
//! **Outside the box.** A glyph's ink overhangs its layout box (the padding,
//! accents, swashes), so the map is defined on the whole plane: the fractions
//! inside `p(·)`, `sin(·)`, the `(1 − v)` / `v` weights and both distortion
//! factors are CLAMPED to `0..=1`, while the `x` and `y` the formulas move are
//! not. So a scaling style keeps scaling an overhang about the centre, a
//! shifting style shifts it with its edge column or row, and every factor
//! stays in `[0.25, 1.75]`. `arc` (stated for `b > 0`; `b < 0` mirrors it,
//! its inner side above the box): `θ` is taken at `u` clamped; on the outer
//! side (`v < 0`) the radius is `R + h·(1 − v)` with `v` unclamped, past
//! `R + h`; on the inner side (`v > 1`, overhang `e = h·(v − 1)`) it is
//! `R²/(R + e)` — `R − e` to first order at the edge (the map is C¹ there),
//! decreasing in `e` and always in `(0, R)`, so no point reaches or passes
//! the centre whatever the overhang and none is mirrored; a point past the
//! box's left or right end adds its signed residual `t = x − clamp(x)` along
//! the tangent at that end's angle: `C + r·n(θ) + t·τ(θ)`, `n` the radial and
//! `τ` the tangent unit vector (the direction of increasing `θ`). Inside the
//! box this is the formula above unchanged.
//!
//! **Bounds** (`Envelope::bounds`, the raster rect of WT-4). Per axis, the
//! pure style's range over a region is found EXACTLY by evaluating the map
//! (distortions 0) on a grid of candidate fractions: the region's two ends,
//! the clamp breakpoints 0 and 1 when inside it, and each style's critical
//! fractions inside it. Why that grid is exact, per style:
//! - `arch`, `upper`, `lower`, `bulge`: `y′` is affine in `p(c)` for a fixed
//!   `v` and piecewise linear in `v` (breakpoints 0 and 1) for a fixed `u`,
//!   so its extremes lie at the extremes of `p(c)` over the region's `u` —
//!   its ends, the breakpoints and `u = ½` — crossed with the `v` ends and
//!   breakpoints; `x′ = x` is extreme at the `u` ends.
//! - `squeeze`: the mirror image across the diagonal — `x′` is affine in
//!   `p(d)`, which is QUADRATIC in `v` with its extreme at `v = ½`, so the
//!   widest row is the middle one and `v = ½` is a candidate; `y′ = y`.
//! - `inflate`: both: `u = ½` for `y′` and `v = ½` for `x′`.
//! - `flag`: `sin(2πu)` is extreme at `u = ¼` and `¾`; `x′ = x`.
//! - `rise`: `sin(π(u − ½))` is monotone on `0..=1`, so ends and breakpoints.
//! - `arc`: inside `0 ≤ u ≤ 1` a coordinate is `r(v)·g(θ(u))` plus the
//!   centre, with `r` monotone in `v` (the inner rule included) and `g` one
//!   of `sin θ`, `cos θ`: the extremes lie at the `v` ends crossed with the
//!   extremes of `g` — the sector's corners, and `θ = 0` (`u = ½`), where the
//!   outer arc reaches furthest from the centre's row. Past an end the angle
//!   is fixed, so a coordinate is `r(v)·const + t·const`, extreme at the end
//!   cap's corners. The inner rule's radii are among the `v` ends.
//!
//! Then the interval product of that range about the centre with the range
//! of the axis's distortion factor over the region's clamped fractions (it is
//! linear in them, so their ends). Exact when `horizontal = vertical = 0`; a
//! cover otherwise.

use crate::outline_limits::MAX_OUTLINE_COORDINATE;

type Point = [f64; 2];

/// Below this `|bend|`, `arc` is the identity: its radius would pass
/// `w·3·10⁵` and its sagitta would round away.
const ARC_IDENTITY_BEND: f64 = 1e-6;

/// The nine envelope styles (the names are Doctored's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeStyle {
    Arch,
    Upper,
    Lower,
    Bulge,
    Squeeze,
    Inflate,
    Flag,
    Rise,
    Arc,
}

impl EnvelopeStyle {
    fn named(name: &str) -> Option<EnvelopeStyle> {
        Some(match name {
            "arch" => EnvelopeStyle::Arch,
            "upper" => EnvelopeStyle::Upper,
            "lower" => EnvelopeStyle::Lower,
            "bulge" => EnvelopeStyle::Bulge,
            "squeeze" => EnvelopeStyle::Squeeze,
            "inflate" => EnvelopeStyle::Inflate,
            "flag" => EnvelopeStyle::Flag,
            "rise" => EnvelopeStyle::Rise,
            "arc" => EnvelopeStyle::Arc,
            _ => return None,
        })
    }

    /// The critical fractions across the box (`u`) at which the pure map's
    /// coordinates turn (module doc, "Bounds").
    fn critical_u(self) -> &'static [f64] {
        match self {
            EnvelopeStyle::Arch
            | EnvelopeStyle::Upper
            | EnvelopeStyle::Lower
            | EnvelopeStyle::Bulge
            | EnvelopeStyle::Inflate
            | EnvelopeStyle::Arc => &[0.5],
            EnvelopeStyle::Flag => &[0.25, 0.75],
            EnvelopeStyle::Squeeze | EnvelopeStyle::Rise => &[],
        }
    }

    /// The critical fractions down the box (`v`): `p(d)`'s turn at `½`.
    fn critical_v(self) -> &'static [f64] {
        match self {
            EnvelopeStyle::Squeeze | EnvelopeStyle::Inflate => &[0.5],
            _ => &[],
        }
    }
}

/// One envelope: a style over a box, with its bend and distortions.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    style: EnvelopeStyle,
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    bend: f64,
    horizontal: f64,
    vertical: f64,
}

/// `t` clamped to `0..=1`.
fn unit(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

/// `p(t) = 1 − t²`.
fn p(t: f64) -> f64 {
    1.0 - t * t
}

impl Envelope {
    /// The envelope of style `style` over the box `rect` (`[x, y, w, h]`).
    /// `Err` for an unknown style; a rect not finite, past
    /// `MAX_OUTLINE_COORDINATE`, or with `w <= 0` or `h <= 0`; and a bend or
    /// distortion that is not finite or outside `-1..=1`.
    pub fn new(
        style: &str,
        rect: [f64; 4],
        bend: f64,
        horizontal: f64,
        vertical: f64,
    ) -> Result<Envelope, String> {
        let style = EnvelopeStyle::named(style).ok_or_else(|| {
            format!(
                "an envelope's style must be one of arch, upper, lower, bulge, squeeze, \
                 inflate, flag, rise, arc (got \"{style}\")"
            )
        })?;
        let [x0, y0, w, h] = rect;
        if rect
            .iter()
            .any(|v| !v.is_finite() || v.abs() > MAX_OUTLINE_COORDINATE)
            || (x0 + w).abs() > MAX_OUTLINE_COORDINATE
            || (y0 + h).abs() > MAX_OUTLINE_COORDINATE
            || w <= 0.0
            || h <= 0.0
        {
            return Err(
                "an envelope's rect must be finite, within 1e9, with a positive \
                        width and height"
                    .into(),
            );
        }
        for (name, value) in [
            ("bend", bend),
            ("horizontal", horizontal),
            ("vertical", vertical),
        ] {
            if !(value.is_finite() && (-1.0..=1.0).contains(&value)) {
                return Err(format!(
                    "an envelope's {name} must be a number from -1 to 1"
                ));
            }
        }
        Ok(Envelope {
            style,
            x0,
            y0,
            w,
            h,
            bend,
            horizontal,
            vertical,
        })
    }

    fn centre(&self) -> Point {
        [self.x0 + self.w * 0.5, self.y0 + self.h * 0.5]
    }

    /// A point's UNCLAMPED fractions across and down the box.
    fn fractions(&self, q: Point) -> (f64, f64) {
        ((q[0] - self.x0) / self.w, (q[1] - self.y0) / self.h)
    }

    /// The image of `q` under the style alone (distortions 0).
    fn pure(&self, q: Point) -> Point {
        let [x, y] = q;
        let (u, v) = self.fractions(q);
        let (uc, vc) = (unit(u), unit(v));
        let (c, d) = (2.0 * uc - 1.0, 2.0 * vc - 1.0);
        let [xc, yc] = self.centre();
        let (b, h2) = (self.bend, self.h * 0.5);
        match self.style {
            EnvelopeStyle::Arch => [x, y - b * h2 * p(c)],
            EnvelopeStyle::Upper => [x, y - b * h2 * p(c) * (1.0 - vc)],
            EnvelopeStyle::Lower => [x, y + b * h2 * p(c) * vc],
            EnvelopeStyle::Bulge => [x, yc + (y - yc) * (1.0 + 0.75 * b * p(c))],
            EnvelopeStyle::Squeeze => [xc + (x - xc) * (1.0 - 0.75 * b * p(d)), y],
            EnvelopeStyle::Inflate => [
                xc + (x - xc) * (1.0 + 0.5 * b * p(d)),
                yc + (y - yc) * (1.0 + 0.5 * b * p(c)),
            ],
            EnvelopeStyle::Flag => [x, y + b * h2 * (std::f64::consts::TAU * uc).sin()],
            EnvelopeStyle::Rise => [x, y - b * h2 * (std::f64::consts::PI * (uc - 0.5)).sin()],
            EnvelopeStyle::Arc => self.arc(q, u, v),
        }
    }

    /// `arc` with the outside-the-box rule (module doc): `θ` at `u` clamped,
    /// the radius from `v` on the outer side and `R²/(R + e)` on the inner,
    /// and an end residual along the end's tangent.
    fn arc(&self, q: Point, u: f64, v: f64) -> Point {
        let b = self.bend;
        if b.abs() < ARC_IDENTITY_BEND {
            return q;
        }
        let x = q[0];
        let magnitude = b.abs();
        let radius = self.w / (std::f64::consts::PI * magnitude);
        let theta = std::f64::consts::PI * magnitude * (unit(u) - 0.5);
        // How far the point lies PAST the inner edge (the one on R), as a
        // fraction of the height: `v − 1` below the box for b > 0, `−v`
        // above it for b < 0; negative on the outer side and inside.
        let (past_inner, centre_y, sign) = if b > 0.0 {
            (v - 1.0, self.y0 + self.h + radius, -1.0)
        } else {
            (-v, self.y0 - radius, 1.0)
        };
        let r = if past_inner > 0.0 {
            let e = self.h * past_inner;
            radius * radius / (radius + e)
        } else {
            radius - self.h * past_inner
        };
        let residual = x - (self.x0 + unit(u) * self.w);
        let (sin, cos) = theta.sin_cos();
        // n = (sin θ, ∓cos θ) and τ = dn/dθ = (cos θ, ±sin θ), y down.
        [
            self.x0 + self.w * 0.5 + r * sin + residual * cos,
            centre_y + sign * r * cos - sign * residual * sin,
        ]
    }

    /// The two distortion factors at `q`'s clamped fractions: across x
    /// (`vertical`, down the box) and across y (`horizontal`, across it).
    fn factors(&self, q: Point) -> (f64, f64) {
        let (u, v) = self.fractions(q);
        let (c, d) = (2.0 * unit(u) - 1.0, 2.0 * unit(v) - 1.0);
        (
            1.0 + 0.75 * self.vertical * d,
            1.0 + 0.75 * self.horizontal * c,
        )
    }

    /// The image of one point (module doc): the style, then the distortions.
    pub fn point(&self, q: Point) -> Point {
        let [x, y] = self.pure(q);
        let [xc, yc] = self.centre();
        let (fx, fy) = self.factors(q);
        [xc + (x - xc) * fx, yc + (y - yc) * fy]
    }

    /// `[min_x, min_y, max_x, max_y]` holding the image of every point of
    /// `region` (`[x, y, w, h]`, `w, h >= 0`): exact when both distortions
    /// are 0, a cover otherwise (module doc, "Bounds"). `Err` for a region
    /// not finite, negative in size or past `MAX_OUTLINE_COORDINATE`, and a
    /// result that is not finite.
    pub fn bounds(&self, region: [f64; 4]) -> Result<[f64; 4], String> {
        let [rx, ry, rw, rh] = region;
        if region
            .iter()
            .any(|v| !v.is_finite() || v.abs() > MAX_OUTLINE_COORDINATE)
            || rw < 0.0
            || rh < 0.0
            || (rx + rw).abs() > MAX_OUTLINE_COORDINATE
            || (ry + rh).abs() > MAX_OUTLINE_COORDINATE
        {
            return Err("an envelope's region must be finite, within 1e9, with a \
                        non-negative width and height"
                .into());
        }
        // The candidate coordinates per axis: the region's ends, then each
        // breakpoint and critical fraction strictly inside it.
        let candidates = |lo: f64, size: f64, origin: f64, extent: f64, critical: &[f64]| {
            let hi = lo + size;
            let mut out = vec![lo, hi];
            for &f in [0.0, 1.0].iter().chain(critical) {
                let at = origin + f * extent;
                if at > lo && at < hi {
                    out.push(at);
                }
            }
            out
        };
        let xs = candidates(rx, rw, self.x0, self.w, self.style.critical_u());
        let ys = candidates(ry, rh, self.y0, self.h, self.style.critical_v());
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for &x in &xs {
            for &y in &ys {
                let image = self.pure([x, y]);
                for axis in 0..2 {
                    low[axis] = low[axis].min(image[axis]);
                    high[axis] = high[axis].max(image[axis]);
                }
            }
        }
        // Each axis's distortion factor is linear in a clamped fraction, so
        // its range over the region is its value at the region's two ends.
        let corners = [[rx, ry], [rx + rw, ry + rh]];
        let (fx0, fy0) = self.factors(corners[0]);
        let (fx1, fy1) = self.factors(corners[1]);
        let centre = self.centre();
        let mut out = [0.0; 4];
        for (axis, (f0, f1)) in [(fx0, fx1), (fy0, fy1)].into_iter().enumerate() {
            let (a, b) = (low[axis] - centre[axis], high[axis] - centre[axis]);
            let products = [a * f0, a * f1, b * f0, b * f1];
            let min = products.iter().copied().fold(f64::INFINITY, f64::min);
            let max = products.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            out[axis] = centre[axis] + min;
            out[axis + 2] = centre[axis] + max;
        }
        if out.iter().any(|v| !v.is_finite()) {
            return Err("an envelope's bounds are not finite".into());
        }
        Ok(out)
    }
}
