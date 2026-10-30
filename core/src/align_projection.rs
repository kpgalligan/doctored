//! Auto-Align Layers' Cylindrical and Spherical projections (phase
//! 2b-2) and the lens-corrected plane of Collage and Reposition (phase 2b-5):
//! the inverse map that reprojects one layer, a pinhole view, onto the
//! reference camera's cylinder or sphere (or through a plane map), its exact
//! forward map, and the destination rect. One value type over plain numbers; the adapters are
//! `doc_align_projection` and `precision_align_projection`, and the resample
//! is `puppet_render::render` through [`InverseMap`].
//!
//! THE CONVENTION. Canvas pixels, x right, y down, pixel centres at
//! half-integers. A layer is a pinhole view with its principal point at
//! `centre` and a focal length of `focal` pixels: its point q sees the ray
//! ((q − centre)/f, 1) of its own camera. `rotation` (row-major 3×3) maps a
//! ray of the REFERENCE camera to the same ray in this layer's camera. The
//! surface is the reference camera's, unrolled at `focal` pixels a radian: a
//! destination point p has the longitude θ = (p.x − origin.x)/f (positive to
//! the right), and
//! - Cylindrical: h = (p.y − origin.y)/f, the ray d = (sin θ, h, cos θ);
//! - Spherical: φ = (p.y − origin.y)/f, d = (sin θ cos φ, sin φ, cos θ cos φ),
//!
//! where `origin` is where the reference's optical axis (θ = 0 and h or
//! φ = 0) lands. The inverse takes r = R·d and q = centre + f·(r.x, r.y)/r.z.
//! The forward map is its exact inverse: d = Rᵀ·((q − centre)/f, 1),
//! θ = atan2(d.x, d.z), and h = d.y / hypot(d.x, d.z) or
//! φ = atan2(d.y, hypot(d.x, d.z)).
//!
//! THE LENS (phase 2b-4). A `distortion` k ≠ 0 makes the layer's
//! pixels the distorted image of the pinhole view: the pinhole point q_u
//! reads the layer at q = `lens_model::radial_map(c, R, q_u, [k, 0, 0], 1)`,
//! c and R the layer rect's `align_distortion::lens_frame`. The inverse
//! refuses a q_u past the first fold (`lens_model::radial_monotone`); the
//! forward map undistorts first (`lens_model::radial_unmap`, whose `None` is
//! the destination rect's [`FAR_SIDE`]). k = 0 takes neither branch, so the
//! map is bit-identical to the lens-free one.
//!
//! THE PLANE (phase 2b-5). Collage and Reposition with Geometric
//! Distortion: `map` (row-major 3×3, a similarity or a translation the host
//! reduced the registration to) takes the layer's CORRECTED canvas point to
//! the destination. The inverse takes u = `unmap`·p (u's w above 0), refuses a
//! u outside the clip rect (each layer keeps its own frame size, as take3's
//! Collage and Reposition D rows show), then reads the layer at the lens's q
//! as above. No focal, rotation, centre or origin: the surfaces' fields are
//! inert on the plane, and `map`/`unmap` are the identity on the surfaces.
//!
//! THE CLIP is Photoshop's index-space clip (phase 2b-5): a destination
//! pixel has content iff its centre's source lies in the layer's PIXEL-CENTRE
//! rect [x0 + ½, x0 + w − ½] × [y0 + ½, y0 + h − ½] (closed). A destination
//! point past the surface (|θ| > π, or Spherical |φ| > π/2), whose ray is
//! behind the layer's camera, or whose source point (and on the plane its
//! corrected point) lies outside that rect has no source and renders
//! transparent. The clip is what gives the projected layer a crisp edge, so
//! its content bounds are the projected rect itself; take2's Cylindrical
//! 974 × 399 is the union of three such rects.
//!
//! THE DESTINATION (phase 2b-5) is the pixel-span of the clip rect's
//! forward image: the pixel-centre perimeter walked every [`PERIMETER_STEP`]
//! (on the plane, its four corners through `map`, a homography mapping its
//! sides to segments) gives lo and hi per axis, and the rect runs from
//! ceil(lo − ½ − [`RECT_SLACK`]) to floor(hi − ½ + [`RECT_SLACK`]) + 1
//! (exclusive), at least one pixel: exactly the pixels whose centres the
//! image reaches. With the reference's axis on a pixel centre (the host's
//! `origin`) this reproduces all twelve of take2's and take3's rows (C rows
//! L/C/R 555/556/555 at x 0/209/419, Spherical 385 and Cylindrical 399 high);
//! the smallest margin is 0.09 px, the right layer's left edge. The Rotate
//! Canvas rule, ceil(bw) × ceil(bh) over the edge extent (2f·atan(½) =
//! 556.38), gives 557 and is rejected, as is 2b-2's outward rounding of the
//! edge extent (556/557/556). The map is continuous and one-to-one on the
//! rays in front of the layer's camera, so the image of the rect is the
//! region its perimeter's image bounds. A perimeter step whose θ jumps by
//! more than π crosses the surface's seam, behind the reference camera (the
//! layer reaches round past the far side; a 360° panorama is not built), and a
//! perimeter ray at a pole, or a plane corner whose w is not above 0, has no
//! image: both refuse [`FAR_SIDE`].
//!
//! EPSILONS, and why:
//! - [`HORIZON`] 1e-9 (relative): a ray with r.z at most 1e-9·|r| lands more
//!   than 1e9 focal lengths from the principal point, outside any layer the
//!   pixel limit admits, so refusing it loses nothing and keeps the division
//!   from overflowing into a point that merely looks finite.
//! - [`POLE`] 1e-12 (relative): a ray whose horizontal part is below 1e-12 of
//!   its length has no longitude worth the name; atan2 of two such numbers is
//!   rounding noise.
//! - [`ORTHONORMAL`] 1e-6 (the largest entry of RRᵀ − I): the host sends a
//!   polar factor converged to 1e-12 and written as JSON decimals, far inside
//!   it, while a skew of 1e-6 moves a point 6000 px out by 0.006 px, which no
//!   one would see; a matrix farther off is not a rotation the host meant.
//! - [`PERIMETER_STEP`] 0.5 px: the forward map's scale is about one
//!   destination pixel per layer pixel, so half-pixel samples of a smooth
//!   curve miss its extreme by the chord error step²/(8ρ), 0.0003 px at a
//!   curvature radius ρ of 100 px.
//! - [`RECT_SLACK`] 0.01 px: the widening of the pixel-span rule, thirty
//!   times that chord error and a ninth of take2's smallest margin (0.09 px),
//!   so a walk that falls just short of a pixel centre the true image reaches
//!   still counts it, and no pixel the image misses by more is added.
use crate::align_distortion::lens_frame;
use crate::doc::MAX_PIXELS;
use crate::lens_model::{radial_map, radial_monotone, radial_unmap};
use crate::puppet_mesh::Rect;
use crate::puppet_render::InverseMap;

/// The ray-behind-the-camera cut, relative to |r|; see the module doc.
const HORIZON: f64 = 1e-9;
/// The no-longitude cut, relative to |d|; see the module doc.
const POLE: f64 = 1e-12;
/// The largest entry of RRᵀ − I a rotation may have; see the module doc.
const ORTHONORMAL: f64 = 1e-6;
/// The perimeter walk's spacing, in layer pixels; see the module doc.
const PERIMETER_STEP: f64 = 0.5;
/// Widening of the pixel-span rule, in px; the half-pixel walk's chord error
/// is step²/(8ρ) (0.0003 px at ρ = 100). See the module doc.
const RECT_SLACK: f64 = 0.01;
/// The 3×3 identity, row-major: `map` and `unmap` on the surfaces.
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// The tail of the refusal a layer that reaches round past the far side
/// answers; the adapters prefix it with the feature and the layer.
pub(crate) const FAR_SIDE: &str = "it reaches round past the far side of the projection.";
pub(crate) const PIXEL_LIMIT_REFUSAL: &str =
    "Auto-Align Layers would make the layer larger than the pixel limit.";

/// The surface a layer is reprojected onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectionKind {
    Cylindrical,
    Spherical,
    Plane,
}

/// One layer's reprojection; see the module doc for every field.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerProjection {
    pub kind: ProjectionKind,
    pub focal: f64,
    pub rotation: [f64; 9],
    pub centre: [f64; 2],
    pub origin: [f64; 2],
    pub source: Rect,
    pub distortion: f64,
    /// The plane's corrected point to destination; identity on the surfaces.
    pub map: [f64; 9],
    /// `map`'s inverse.
    pub unmap: [f64; 9],
}

fn rotate(m: &[f64; 9], v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn rotate_transposed(m: &[f64; 9], v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[3] * v[1] + m[6] * v[2],
        m[1] * v[0] + m[4] * v[1] + m[7] * v[2],
        m[2] * v[0] + m[5] * v[1] + m[8] * v[2],
    ]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The homography `m` at `p`, or `None` where its w is not above 0 or the
/// result is not finite.
fn apply(m: &[f64; 9], p: [f64; 2]) -> Option<[f64; 2]> {
    let w = m[6] * p[0] + m[7] * p[1] + m[8];
    if w.is_nan() || w <= 0.0 {
        return None;
    }
    let out = [
        (m[0] * p[0] + m[1] * p[1] + m[2]) / w,
        (m[3] * p[0] + m[4] * p[1] + m[5]) / w,
    ];
    (out[0].is_finite() && out[1].is_finite()).then_some(out)
}

/// The pixel-span rule along one axis (the module doc): the first pixel and
/// the count whose centres lie in [lo, hi], widened by [`RECT_SLACK`], at
/// least one.
fn pixel_span(lo: f64, hi: f64) -> (i64, u32) {
    let first = (lo - 0.5 - RECT_SLACK).ceil() as i64;
    let end = ((hi - 0.5 + RECT_SLACK).floor() as i64 + 1).max(first + 1);
    (first, u32::try_from(end - first).unwrap_or(u32::MAX))
}

impl LayerProjection {
    /// The projection, refusing a focal that is not finite or not above 0
    /// and a rotation that is not finite, not orthonormal within
    /// [`ORTHONORMAL`] or a reflection. `centre` and `origin` must be finite;
    /// `distortion` is the lens coefficient (module doc), checked by the
    /// adapter (`align_distortion::folds`).
    pub(crate) fn new(
        kind: ProjectionKind,
        focal: f64,
        rotation: [f64; 9],
        centre: [f64; 2],
        origin: [f64; 2],
        source: Rect,
        distortion: f64,
    ) -> Result<Self, String> {
        if !focal.is_finite() || focal <= 0.0 {
            return Err("project layer focal must be a finite number above 0".into());
        }
        if !centre.iter().chain(&origin).all(|v| v.is_finite()) {
            return Err("project layer centre and origin must be finite".into());
        }
        let refusal = "project layer rotation must be an orthonormal 3x3 matrix with determinant 1";
        if !rotation.iter().all(|v| v.is_finite()) {
            return Err(refusal.into());
        }
        let r = &rotation;
        for i in 0..3 {
            for j in 0..3 {
                let dot: f64 = (0..3).map(|k| r[i * 3 + k] * r[j * 3 + k]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                if (dot - want).abs() > ORTHONORMAL {
                    return Err(refusal.into());
                }
            }
        }
        let det = r[0] * (r[4] * r[8] - r[5] * r[7]) - r[1] * (r[3] * r[8] - r[5] * r[6])
            + r[2] * (r[3] * r[7] - r[4] * r[6]);
        if det <= 0.0 {
            return Err(refusal.into());
        }
        Ok(Self {
            kind,
            focal,
            rotation,
            centre,
            origin,
            source,
            distortion,
            map: IDENTITY,
            unmap: IDENTITY,
        })
    }

    /// The plane (module doc): `map` takes the layer's CORRECTED canvas point
    /// to the destination. Refuses a map that is not finite or whose
    /// determinant is not above 0 (or whose inverse is not finite).
    /// `distortion` is checked by the adapter, as on the surfaces.
    pub(crate) fn plane(map: [f64; 9], source: Rect, distortion: f64) -> Result<Self, String> {
        let refusal = "project layer map must be invertible with a positive determinant";
        if !map.iter().all(|v| v.is_finite()) {
            return Err(refusal.into());
        }
        let m = &map;
        let adjugate = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let det = m[0] * adjugate[0] + m[1] * adjugate[3] + m[2] * adjugate[6];
        if det.is_nan() || det <= 0.0 {
            return Err(refusal.into());
        }
        let unmap = adjugate.map(|v| v / det);
        if !unmap.iter().all(|v| v.is_finite()) {
            return Err(refusal.into());
        }
        Ok(Self {
            kind: ProjectionKind::Plane,
            focal: 0.0,
            rotation: IDENTITY,
            centre: [0.0, 0.0],
            origin: [0.0, 0.0],
            source,
            distortion,
            map,
            unmap,
        })
    }

    /// Whether the canvas point `q` lies in the layer's pixel-centre rect
    /// (closed): the module doc's clip.
    fn inside(&self, q: [f64; 2]) -> bool {
        let (x, y, w, h) = self.source;
        let (x0, y0) = (x as f64 + 0.5, y as f64 + 0.5);
        let (x1, y1) = (x as f64 + f64::from(w) - 0.5, y as f64 + f64::from(h) - 0.5);
        q[0] >= x0 && q[1] >= y0 && q[0] <= x1 && q[1] <= y1
    }

    /// The pinhole (on the plane, corrected) point the destination point `p`
    /// comes from, before the lens and the clip of the source point.
    fn pinhole(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        if self.kind == ProjectionKind::Plane {
            let u = apply(&self.unmap, p)?;
            return self.inside(u).then_some(u);
        }
        let f = self.focal;
        let theta = (p[0] - self.origin[0]) / f;
        let down = (p[1] - self.origin[1]) / f;
        if theta.is_nan() || theta.abs() > std::f64::consts::PI {
            return None;
        }
        let d = match self.kind {
            ProjectionKind::Cylindrical | ProjectionKind::Plane => [theta.sin(), down, theta.cos()],
            ProjectionKind::Spherical => {
                if down.is_nan() || down.abs() > std::f64::consts::FRAC_PI_2 {
                    return None;
                }
                [
                    theta.sin() * down.cos(),
                    down.sin(),
                    theta.cos() * down.cos(),
                ]
            }
        };
        let r = rotate(&self.rotation, d);
        if r[2].is_nan() || r[2] <= HORIZON * length(r) {
            return None;
        }
        Some([
            self.centre[0] + f * r[0] / r[2],
            self.centre[1] + f * r[1] / r[2],
        ])
    }

    /// The canvas point of the layer that the destination point `p` comes
    /// from, or `None` where the module doc's clip says there is none.
    pub(crate) fn inverse(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let mut q = self.pinhole(p)?;
        if self.distortion != 0.0 {
            let (c, radius) = lens_frame(self.source);
            let (x, y) = ((q[0] - c[0]) / radius, (q[1] - c[1]) / radius);
            let radial = [self.distortion, 0.0, 0.0];
            if !radial_monotone(x * x + y * y, radial) {
                return None;
            }
            q = radial_map(c, radius, q, radial, 1.0);
        }
        (q[0].is_finite() && q[1].is_finite() && self.inside(q)).then_some(q)
    }

    /// The destination point the layer's canvas point `q` goes to, the exact
    /// inverse of [`Self::inverse`]; `None` for a ray at a pole, a point with
    /// no corrected preimage, or on the plane a point whose w is not above 0.
    pub(crate) fn forward(&self, q: [f64; 2]) -> Option<[f64; 2]> {
        let f = self.focal;
        let q = if self.distortion != 0.0 {
            let (c, radius) = lens_frame(self.source);
            radial_unmap(c, radius, q, [self.distortion, 0.0, 0.0])?
        } else {
            q
        };
        if self.kind == ProjectionKind::Plane {
            return apply(&self.map, q);
        }
        let ray = [
            (q[0] - self.centre[0]) / f,
            (q[1] - self.centre[1]) / f,
            1.0,
        ];
        let d = rotate_transposed(&self.rotation, ray);
        let around = d[0].hypot(d[2]);
        if around.is_nan() || around <= POLE * length(d) {
            return None;
        }
        let theta = d[0].atan2(d[2]);
        let down = match self.kind {
            ProjectionKind::Cylindrical | ProjectionKind::Plane => d[1] / around,
            ProjectionKind::Spherical => d[1].atan2(around),
        };
        let p = [self.origin[0] + f * theta, self.origin[1] + f * down];
        (p[0].is_finite() && p[1].is_finite()).then_some(p)
    }

    /// The destination rect: the pixel-span of the clip rect's image (the
    /// module doc). Refuses [`FAR_SIDE`] for a perimeter that crosses the
    /// seam or meets a pole (on the plane, a corner whose w is not above 0),
    /// and [`PIXEL_LIMIT_REFUSAL`] for a rect past `MAX_PIXELS` or an origin
    /// past i32.
    pub(crate) fn dest_rect(&self) -> Result<Rect, String> {
        let (x, y, w, h) = self.source;
        let (x0, y0) = (x as f64 + 0.5, y as f64 + 0.5);
        let (x1, y1) = (x as f64 + f64::from(w) - 0.5, y as f64 + f64::from(h) - 0.5);
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let mut points: Vec<[f64; 2]> = Vec::new();
        if self.kind == ProjectionKind::Plane {
            for corner in corners {
                points.push(apply(&self.map, corner).ok_or(FAR_SIDE)?);
            }
        } else {
            let mut last_theta: Option<f64> = None;
            for side in 0..4 {
                let (a, b) = (corners[side], corners[(side + 1) % 4]);
                let span = (b[0] - a[0]).abs().max((b[1] - a[1]).abs());
                let steps = ((span / PERIMETER_STEP).ceil() as usize).max(1);
                for k in 0..steps {
                    let t = k as f64 / steps as f64;
                    let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    let p = self.forward(q).ok_or(FAR_SIDE)?;
                    let theta = (p[0] - self.origin[0]) / self.focal;
                    if last_theta.is_some_and(|l| (theta - l).abs() > std::f64::consts::PI) {
                        return Err(FAR_SIDE.into());
                    }
                    last_theta = Some(theta);
                    points.push(p);
                }
            }
            // The walk closes on the first corner.
            let first = (points[0][0] - self.origin[0]) / self.focal;
            if last_theta.is_some_and(|l| (first - l).abs() > std::f64::consts::PI) {
                return Err(FAR_SIDE.into());
            }
        }
        // Past i32 either way is past the limit, and keeps the span's integer
        // arithmetic from overflowing.
        if points
            .iter()
            .flatten()
            .any(|v| v.abs() > f64::from(i32::MAX))
        {
            return Err(PIXEL_LIMIT_REFUSAL.into());
        }
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for p in &points {
            for k in 0..2 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        let (dx, dw) = pixel_span(low[0], high[0]);
        let (dy, dh) = pixel_span(low[1], high[1]);
        if u64::from(dw) * u64::from(dh) > MAX_PIXELS
            || i32::try_from(dx).is_err()
            || i32::try_from(dy).is_err()
        {
            return Err(PIXEL_LIMIT_REFUSAL.into());
        }
        Ok((dx, dy, dw, dh))
    }
}

impl InverseMap for LayerProjection {
    fn source_point(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        self.inverse(p)
    }
}
