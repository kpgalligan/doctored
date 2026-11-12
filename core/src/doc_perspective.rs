//! Perspective transform: one layer resampled so its rect lands on an
//! arbitrary convex quad — the projective generalization of `doc_transform`'s
//! affine pipeline, which owns the shared resamplers, extent rounding and
//! epsilon conventions this module reuses.
//!
//! The public currency is the destination QUAD, not a 3x3 matrix: the four
//! canvas points the layer rect's corners land on, in the rect's own corner
//! order TL, TR, BR, BL (clockwise on a y-down canvas). Hosts drag corners
//! and agents think in corners; the homography that realizes the mapping is
//! an implementation detail solved here, so no caller carries projective
//! math and no matrix element-order convention has to cross the FFI.
//!
//! A quad that is a parallelogram (within `doc_transform`'s canvas-pixel
//! epsilon) IS an affine transform, and is delegated to
//! [`DrDocument::transform_layer`] — inheriting its lossless exact paths, so
//! an integer translation expressed as corners still costs no resample.

use image::imageops::FilterType;
use std::sync::Arc;

use crate::doc::{DrDocument, LayerRaster};
use crate::doc_lock::EditKind;
use crate::doc_transform::{
    bounds_of_corners, resample_mask_outside, resample_rgba, Affine, Sampler, SourceMap,
    EXACT_EPSILON, MIN_DETERMINANT,
};
use crate::style::{quad_mean_scale, scaled_style};

/// Smallest homogeneous w the forward map may give a source corner, with the
/// solve normalized so w = 1 at the rect's top-left. The four corner w's are
/// `1`, `1 + g`, `1 + h` and `1 + g + h` in the unit-square frame; one at or
/// below zero means the quad is concave or self-intersecting — the map folds
/// through the horizon inside the layer — which is refused rather than
/// rendered as a fold. The threshold is scale-invariant because the
/// normalization pins w's unit, and 1e-6 (rather than 0) keeps a corner from
/// sitting so close to the horizon that its pixels stretch without bound.
const MIN_CORNER_W: f64 = 1e-6;

/// A row-major 3x3 projective matrix in canvas space: row 0 is the x
/// numerator, row 1 the y numerator, row 2 the denominator. The typed geometry
/// planner shares this internal model; quads still cross the FFI, and the
/// resamplers take the raw array.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Homography(pub(crate) [f64; 9]);

impl Homography {
    /// Inverse oriented to the visible side of the projective horizon.
    pub(crate) fn visible_inverse(&self) -> Option<[f64; 9]> {
        let mut inverse = self.adjugate();
        if !inverse.is_finite() {
            return None;
        }
        if self.determinant() < 0.0 {
            for v in &mut inverse.0 {
                *v = -*v;
            }
        }
        Some(inverse.0)
    }
    pub(crate) fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6])
    }

    /// The adjugate — a scale of the inverse, which is all a projective map
    /// needs (numerator and denominator share the scale, so it cancels in
    /// the divide). Using it directly skips the division by the determinant
    /// and the precision it would cost.
    fn adjugate(&self) -> Homography {
        let m = &self.0;
        Homography([
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ])
    }

    fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

/// Solves the homography mapping the axis-aligned rect `(x0, y0, w, h)` onto
/// `quad` (TL, TR, BR, BL canvas points), normalized so the rect's top-left
/// has homogeneous w = 1.
///
/// The classic 4-point closed form (Heckbert) maps the UNIT square to the
/// quad; the rect is first taken to the unit square by an exact affine, and
/// the two compose. `None` when the quad's corner w's fall below
/// [`MIN_CORNER_W`] (concave or self-intersecting), when the quad collapses
/// (determinant below [`MIN_DETERMINANT`] in the unit-square frame, where
/// with w pinned to 1 the determinant is area-LIKE — exactly the area when
/// `g = h = 0` — so the floor refuses quads whose area is under a
/// billionth of a square pixel, a few hundred-thousandths of a pixel
/// across), or when any intermediate is not finite.
pub(crate) fn solve_rect_to_quad(
    rect: (f64, f64, f64, f64),
    quad: &[f64; 8],
) -> Option<Homography> {
    let (rx, ry, rw, rh) = rect;
    if rw <= 0.0 || rh <= 0.0 {
        return None;
    }
    let [x0, y0, x1, y1, x2, y2, x3, y3] = *quad;
    // Second differences: zero exactly when the quad is a parallelogram.
    let dx3 = x0 - x1 + x2 - x3;
    let dy3 = y0 - y1 + y2 - y3;

    // Unit square -> quad, corners (0,0)->TL, (1,0)->TR, (1,1)->BR,
    // (0,1)->BL, in the form [a b c; d e f; g h 1].
    let (g, h);
    if dx3.abs() <= EXACT_EPSILON && dy3.abs() <= EXACT_EPSILON {
        g = 0.0;
        h = 0.0;
    } else {
        let dx1 = x1 - x2;
        let dy1 = y1 - y2;
        let dx2 = x3 - x2;
        let dy2 = y3 - y2;
        let den = dx1 * dy2 - dx2 * dy1;
        if den.abs() < MIN_DETERMINANT {
            return None;
        }
        g = (dx3 * dy2 - dx2 * dy3) / den;
        h = (dx1 * dy3 - dx3 * dy1) / den;
    }
    // Corner w's in the normalized frame; at or below zero the quad is
    // concave/self-intersecting and the map folds inside the rect.
    for w in [1.0, 1.0 + g, 1.0 + h, 1.0 + g + h] {
        if w < MIN_CORNER_W || w.is_nan() {
            return None;
        }
    }
    let unit = Homography([
        x1 - x0 + g * x1,
        x3 - x0 + h * x3,
        x0,
        y1 - y0 + g * y1,
        y3 - y0 + h * y3,
        y0,
        g,
        h,
        1.0,
    ]);
    if !unit.is_finite() || unit.determinant().abs() < MIN_DETERMINANT {
        return None;
    }
    // Compose with rect -> unit square: (x, y) -> ((x-rx)/rw, (y-ry)/rh).
    // Multiplying columns directly keeps it exact where the inputs are.
    let m = &unit.0;
    let full = Homography([
        m[0] / rw,
        m[1] / rh,
        m[2] - m[0] * rx / rw - m[1] * ry / rh,
        m[3] / rw,
        m[4] / rh,
        m[5] - m[3] * rx / rw - m[4] * ry / rh,
        m[6] / rw,
        m[7] / rh,
        m[8] - m[6] * rx / rw - m[7] * ry / rh,
    ]);
    full.is_finite().then_some(full)
}

/// The affine a parallelogram quad denotes: the rect's TL, TR and BL corners
/// pin all six elements (BR is implied, which is what "parallelogram" means).
pub(crate) fn parallelogram_affine(rect: (f64, f64, f64, f64), quad: &[f64; 8]) -> Affine {
    let (rx, ry, rw, rh) = rect;
    let [x0, y0, x1, y1, _, _, x3, y3] = *quad;
    let a = (x1 - x0) / rw;
    let b = (y1 - y0) / rw;
    let c = (x3 - x0) / rh;
    let d = (y3 - y0) / rh;
    Affine {
        a,
        b,
        c,
        d,
        tx: x0 - a * rx - c * ry,
        ty: y0 - b * rx - d * ry,
    }
}

/// Bounds for independent source planes under the already validated forward
/// homography. A plane extending across the horizon is refused as a whole.
pub(crate) fn transformed_projective_bounds(
    size: (u32, u32),
    offset: (i32, i32),
    forward: &[f64; 9],
) -> Option<(i32, i32, u32, u32)> {
    bounds_of_corners(project_corners(size, offset, forward)?)
}

/// The plane rect's four corners (TL, TR, BL, BR) through the forward
/// homography. `None` when a corner's homogeneous w falls below
/// [`MIN_CORNER_W`], i.e. the rect reaches the horizon.
pub(crate) fn project_corners(
    size: (u32, u32),
    offset: (i32, i32),
    forward: &[f64; 9],
) -> Option<[(f64, f64); 4]> {
    let (x, y) = (f64::from(offset.0), f64::from(offset.1));
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let mut corners = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)];
    for corner in &mut corners {
        let (x, y) = *corner;
        let den = forward[6] * x + forward[7] * y + forward[8];
        if den < MIN_CORNER_W {
            return None;
        }
        *corner = (
            (forward[0] * x + forward[1] * y + forward[2]) / den,
            (forward[3] * x + forward[4] * y + forward[5]) / den,
        );
    }
    Some(corners)
}

impl DrDocument {
    /// Transforms layer `idx` so its rect (offset plus pixel dimensions, in
    /// canvas coordinates) lands corner-for-corner on `quad` — four canvas
    /// points in the order TL, TR, BR, BL of the SOURCE rect. The layer's
    /// new offset and size are the quad's bounding box, rounded outward with
    /// the same integer snapping as `transform_layer`; pixels are resampled
    /// by inverse-mapping destination pixel centres through the solved
    /// homography, with `transform_layer`'s premultiplied kernels. Extent
    /// pixels outside the quad come out fully transparent, as do samples
    /// beyond the source. A layer MASK rides along with the same map, extent
    /// and kernel; everything else about the layer — name, opacity, blend
    /// mode, visibility, mask-enabled flag, `meta` — survives, exactly as in
    /// `transform_layer`.
    ///
    /// A PARALLELOGRAM quad (opposite sides equal within the shared 1e-9
    /// canvas-pixel epsilon) is an affine transform and delegates to
    /// [`DrDocument::transform_layer`], keeping its lossless exact paths: an
    /// integer translation handed over as corners is still a plain pixel
    /// copy.
    ///
    /// Returns `None` when `idx` is out of range, any quad coordinate is not
    /// finite, the quad is concave or self-intersecting (the fold check in
    /// [`solve_rect_to_quad`]), the quad collapses toward zero area, the
    /// filter is unknown, or the destination extent is empty, outside the
    /// i32 offset range, or over the pixel budget — the same refusal surface
    /// as `transform_layer`, with the quad shape checks in place of the
    /// determinant check.
    pub fn perspective_layer(
        &self,
        idx: usize,
        quad: [f64; 8],
        filter: FilterType,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Position, |doc| {
            doc.perspective_layer_unlocked(idx, quad, filter)
        })
    }

    /// The body of [`Self::perspective_layer`], outside the lock gate. Split
    /// out only so the gate is one line; that method is its only caller.
    fn perspective_layer_unlocked(
        &self,
        idx: usize,
        quad: [f64; 8],
        filter: FilterType,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        if !quad.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (lw, lh) = layer.pixels.dimensions();
        let rect = (
            f64::from(layer.offset.0),
            f64::from(layer.offset.1),
            f64::from(lw),
            f64::from(lh),
        );
        let dx3 = quad[0] - quad[2] + quad[4] - quad[6];
        let dy3 = quad[1] - quad[3] + quad[5] - quad[7];
        if dx3.abs() <= EXACT_EPSILON && dy3.abs() <= EXACT_EPSILON {
            return self.transform_layer(idx, parallelogram_affine(rect, &quad), filter);
        }
        let sampler = Sampler::from_filter(filter)?;
        let forward = solve_rect_to_quad(rect, &quad)?;
        let inverse = Homography(forward.visible_inverse()?);
        // The extent comes from the quad itself — it IS the corners' image,
        // so no solve residue re-enters the rounding.
        let dest = bounds_of_corners([
            (quad[0], quad[1]),
            (quad[2], quad[3]),
            (quad[4], quad[5]),
            (quad[6], quad[7]),
        ])?;
        let (dx, dy, _, _) = dest;
        if !self.native_layers_fit_dimensions(self.layers.iter().enumerate().map(|(i, l)| {
            if i == idx {
                (dest.2, dest.3)
            } else {
                l.pixels.dimensions()
            }
        })) {
            return None;
        }
        let mask_dest = match layer.mask.as_ref().filter(|_| layer.mask_properties.linked) {
            Some(mask) => {
                let mdest = transformed_projective_bounds(
                    mask.dimensions(),
                    layer.mask_properties.offset,
                    &forward.0,
                )?;
                if !layer.mask_properties.valid((mdest.2, mdest.3)) {
                    return None;
                }
                if !self.native_masks_fit_dimensions(self.layers.iter().enumerate().filter_map(
                    |(i, l)| {
                        l.mask.as_ref().map(|m| {
                            if i == idx {
                                (mdest.2, mdest.3)
                            } else {
                                m.dimensions()
                            }
                        })
                    },
                )) {
                    return None;
                }
                Some(mdest)
            }
            None => None,
        };
        // Both independent destinations and aggregate capacities passed before
        // either large raster buffer is allocated.
        let map = SourceMap::projective(&inverse.0, layer.offset, (dx, dy));
        let pixels = {
            let image = layer.pixels.to_contiguous().ok()?;
            LayerRaster::unseeded(resample_rgba(
                &image,
                dest,
                &map,
                sampler,
                self.linear_light.as_deref(),
            ))
        };
        let (mask, mask_offset) = if let (Some(raw), Some(mdest)) = (&layer.mask, mask_dest) {
            let offset = (mdest.0, mdest.1);
            let map = SourceMap::projective(&inverse.0, layer.mask_properties.offset, offset);
            (
                Some(Arc::new(
                    resample_mask_outside(
                        raw,
                        mdest,
                        &map,
                        sampler,
                        layer.mask_properties.outside_coverage,
                        crate::job_control::JobContext::NONE,
                    )
                    .ok()?,
                )),
                offset,
            )
        } else {
            (layer.mask.clone(), layer.mask_properties.offset)
        };

        let mut doc = self.clone();
        let target = doc.layers.get_mut(idx)?;
        target.pixels = pixels;
        target.mask = mask;
        target.mask_properties.offset = mask_offset;
        target.offset = (dx, dy);
        // "Scale Effects" by the area-based mean scale (the parallelogram
        // early return above scales through transform_layer itself).
        target.style = scaled_style(&target.style, quad_mean_scale(quad, lw, lh));
        Some(doc)
    }
}
