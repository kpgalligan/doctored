//! FREE TRANSFORM OF THE SELECTED PIXELS: the geometry half of
//! `SelectedPixelsEdit::TransformSelected`. The float, the hole, the
//! source-over landing and the single quantize are the move's (`selected_pixels`);
//! this module only says WHERE the float lands and what it looks like there.
//!
//! The contract (measured against Photoshop 26.10, `Trnf` on a
//! synthetic 48×16 image): the float is the layer × the selection
//! coverage, it is resampled through the box's affine or perspective map, the
//! hole is `a·(1−c)` with the RGB kept, the float merges back with Normal
//! source-over clipped to the canvas, the mask stays put, and the transformed
//! coverage becomes the selection.
//!
//! * The float is resampled as PREMULTIPLIED f32 (`[r·a·c, g·a·c, b·a·c,
//!   a·c]`, [`float_sample`]) through `sampling::resample_into`, the one
//!   spatial walk every transform shares, with transparent taps outside F.
//!   Each landed sample is unpremultiplied and THEN clamped
//!   ([`landing_sample`], `sampling::straight_unorm`'s order, so a negative
//!   Lanczos/bicubic lobe cannot shift color), and reaches `moved_pixel` as a
//!   float of coverage 1: its alpha already carries `a·c`.
//! * At 32-bit float the landed sample goes through [`landing_extended`] instead:
//!   `sampling::straight_extended` against the float's own visible range (the RGB of F's pixels
//!   with `a·c > 0`, `precision_sampling::visible_range`), `float_document`'s D3 clamp, so ringing
//!   never leaves the float's range and alpha below 2^-24 drops to transparent. Nothing quantizes.
//! * THE LANDING REACH. `resample_into` samples a fixed `taps` window around
//!   the inverse-mapped centre `(u, v)` and never widens it. A destination can
//!   therefore receive a non-zero tap from F only when `(u, v)` lies within
//!   `Kernel::radius()` source pixels of F (0 for nearest). [`SelectedTransform::landing`]
//!   grows F by that radius on every side, projects the grown corners and
//!   takes their outward-rounded box: a superset of every destination with a
//!   tap into F, which is also exactly where the companion selection
//!   transform (`precision_sampling::transform_plane` over the whole canvas)
//!   can give non-zero coverage. So wherever the new selection selects, the
//!   pixels landed. When only the GROWN corners reach the horizon, the whole
//!   canvas is the landing (the box would be unbounded); F's own corners
//!   reaching it is refused.
//! * NO WORK BOUND. The pixel landing and the companion selection op
//!   (`PlaneOperation::plan` over the canvas, run by [`SelectedTransform::resolve`]
//!   for its validation; its own error passes through) both run under the
//!   caller's job control, bounded by the plan's memory and Cancel. At commit
//!   (UI and MCP) the host plans the pixel edit first and then runs the pixel
//!   edit and the selection transform in ONE background job.
//! * Exact forms stay exact: identity is no edit, an integer translation IS
//!   the move (bit for bit), and the other exact axis-aligned forms sample
//!   with nearest, a pure permutation.
//!
//! `SelectedPixelsEdit::TransformSelected`
//! (`{"op":"transform_selected",layer,matrix|source_rect+quad,filter}`) on the move's walk: `CanvasMap`
//! (affine, or `solve_rect_to_quad`'s homography; a parallelogram quad is its affine),
//! `SelectedTransform::resolve` (identity → no edit, an integer translation IS the move, other
//! `exact_form`s sample nearest) and the premultiplied f32 float (`float_sample`) resampled by
//! `sampling::resample_into` and unpremultiplied-then-clamped (`landing_sample`) into
//! `moved_pixel`. The landing (`landing_box`) is F grown by `Kernel::radius()` then projected
//! (`doc_transform::corner_box` / `doc_perspective::project_corners`), so it covers every pixel the
//! companion selection transform selects. The sample-free plan charges
//! `SelectedPixelsEdit::landing_scratch`: 16 bytes per pixel of `landing_box(layer ∩
//! canvas)`, a monotone bound on landing(F) since F ⊆ layer ∩ canvas; 0 for identity/integer
//! translation/off-canvas layer, the whole canvas at the horizon. The companion is the
//! `PlaneOperation` plan (`transform`, or the `perspective` kind this adds to
//! `precision_selection_plane.rs`); neither it nor the landing refuses for work. Family `selected_pixels_transform`; admitted at 32-bit float for a layer target.
//! Tests: `tests/transform_selected_pixels.rs`.

use crate::doc_perspective::{
    parallelogram_affine, project_corners, solve_rect_to_quad, Homography,
};
use crate::doc_transform::{
    corner_box, exact_form, Affine, Exact, Sampler, SourceMap, EXACT_EPSILON, MIN_DETERMINANT,
};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::precision_sampling::GeometryFilter;
use crate::precision_selection_plane::PlaneOperation;
use crate::sampling::resample_into;
use crate::selected_pixels::CanvasRect;

pub(crate) const EMPTY_AREA: &str = "The selection holds no pixels to transform.";
const DEGENERATE: &str = "The transform matrix is degenerate or not finite.";
const NOT_CONVEX: &str = "The perspective corners must form a convex quad.";
const HORIZON: &str = "The perspective map folds the selected pixels past its horizon.";

/// The canvas-space map the float's box goes through.
pub(crate) enum CanvasMap {
    Affine(Affine),
    Projective(Homography),
}

impl CanvasMap {
    /// Finite and invertible (`|det| ≥ MIN_DETERMINANT`), else refused.
    pub(crate) fn affine(m: [f64; 6]) -> Result<Self, String> {
        let affine = Affine::from_array(m);
        let det = affine.determinant();
        if !affine.is_finite() || !det.is_finite() || det.abs() < MIN_DETERMINANT {
            return Err(DEGENERATE.into());
        }
        Ok(Self::Affine(affine))
    }

    /// A parallelogram (second differences ≤ EXACT_EPSILON) → parallelogram_affine; else solve_rect_to_quad.
    pub(crate) fn quad(source_rect: [f64; 4], quad: [f64; 8]) -> Result<Self, String> {
        let [x, y, w, h] = source_rect;
        if !source_rect.iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
            return Err(DEGENERATE.into());
        }
        if !quad.iter().all(|v| v.is_finite()) {
            return Err(NOT_CONVEX.into());
        }
        let rect = (x, y, w, h);
        let [x0, y0, x1, y1, x2, y2, x3, y3] = quad;
        // `solve_rect_to_quad`'s own parallelogram test, so the two agree.
        if (x0 - x1 + x2 - x3).abs() <= EXACT_EPSILON && (y0 - y1 + y2 - y3).abs() <= EXACT_EPSILON
        {
            let m = parallelogram_affine(rect, &quad);
            return Self::affine([m.a, m.b, m.c, m.d, m.tx, m.ty]);
        }
        solve_rect_to_quad(rect, &quad)
            .map(Self::Projective)
            .ok_or_else(|| NOT_CONVEX.into())
    }

    /// Destination pixel indices (from `dest_origin`) → coordinates in the
    /// source plane standing at `source_offset`.
    pub(crate) fn source_map(
        &self,
        source_offset: (i32, i32),
        dest_origin: (i32, i32),
    ) -> Result<SourceMap, String> {
        match self {
            Self::Affine(m) => {
                let inverse = m.inverse().ok_or(DEGENERATE)?;
                Ok(SourceMap::new(&inverse, source_offset, dest_origin))
            }
            Self::Projective(h) => {
                let inverse = h.visible_inverse().ok_or(NOT_CONVEX)?;
                Ok(SourceMap::projective(&inverse, source_offset, dest_origin))
            }
        }
    }

    /// `rect`'s four corners through the map; `None` when a projective corner
    /// reaches the horizon (`doc_perspective::project_corners`).
    pub(crate) fn corners(&self, rect: CanvasRect) -> Option<[(f64, f64); 4]> {
        match self {
            Self::Affine(m) => {
                let (x0, y0) = (rect.0 as f64, rect.1 as f64);
                let (x1, y1) = (x0 + f64::from(rect.2), y0 + f64::from(rect.3));
                Some([
                    m.apply(x0, y0),
                    m.apply(x1, y0),
                    m.apply(x0, y1),
                    m.apply(x1, y1),
                ])
            }
            Self::Projective(h) => {
                let offset = (i32::try_from(rect.0).ok()?, i32::try_from(rect.1).ok()?);
                project_corners((rect.2, rect.3), offset, &h.0)
            }
        }
    }
}

/// A resolved, non-identity selected-pixels transform.
pub(crate) struct SelectedTransform {
    pub(crate) map: CanvasMap,
    pub(crate) sampler: Sampler,
}

impl SelectedTransform {
    /// Ok(None) = identity. An exact_form Geom forces Sampler::Nearest. Unless exact_form is Translate,
    /// it also runs the COMPANION selection op's plan over `canvas`: PlaneOperation::Transform{matrix,
    /// filter} or Perspective{source_rect, quad, filter}, with the REQUESTED filter. That plan's own
    /// error passes through; it has no work refusal.
    pub(crate) fn resolve(
        matrix: Option<[f64; 6]>,
        source_rect: Option<[f64; 4]>,
        quad: Option<[f64; 8]>,
        filter: GeometryFilter,
        canvas: (u32, u32),
    ) -> Result<Option<Self>, String> {
        let (map, companion) = match (matrix, source_rect, quad) {
            (Some(matrix), None, None) => (
                CanvasMap::affine(matrix)?,
                PlaneOperation::Transform { matrix, filter },
            ),
            (None, Some(source_rect), Some(quad)) => (
                CanvasMap::quad(source_rect, quad)?,
                PlaneOperation::Perspective {
                    source_rect,
                    quad,
                    filter,
                },
            ),
            _ => return Err(SHAPE.into()),
        };
        let mut transform = Self {
            map,
            sampler: filter.sampler(),
        };
        if let CanvasMap::Affine(m) = &transform.map {
            match exact_form(m) {
                Some(Exact::Translate) => {
                    if m.tx.round() == 0.0 && m.ty.round() == 0.0 {
                        return Ok(None);
                    }
                    transform.sampler = Sampler::Nearest;
                    if transform.translation().is_some() {
                        return Ok(Some(transform));
                    }
                }
                Some(Exact::Geom(_)) => transform.sampler = Sampler::Nearest,
                None => {}
            }
        }
        companion.plan(canvas)?;
        Ok(Some(transform))
    }

    /// exact_form Translate → MoveSelected delta (when it fits i32).
    pub(crate) fn translation(&self) -> Option<(i32, i32)> {
        let CanvasMap::Affine(m) = &self.map else {
            return None;
        };
        if !matches!(exact_form(m), Some(Exact::Translate)) {
            return None;
        }
        let fit = |v: f64| {
            let v = v.round();
            (v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX)).then_some(v as i32)
        };
        Some((fit(m.tx)?, fit(m.ty)?))
    }

    /// reach = Kernel::radius() source px (0 for Nearest). Grow `float` by reach on every side, project
    /// its corners, take corner_box, then ∩ canvas. Ok(None) when that misses the canvas.
    /// Err(horizon) when `float`'s own corners do not project. When only the GROWN corners do not
    /// project, use the whole canvas.
    pub(crate) fn landing_box(
        &self,
        float: CanvasRect,
        canvas: (u32, u32),
    ) -> Result<Option<CanvasRect>, String> {
        let reach = match self.sampler {
            Sampler::Nearest => 0,
            Sampler::Kernel(kernel) => kernel.radius(),
        };
        self.map.corners(float).ok_or(HORIZON)?;
        let grown = (
            float.0 - reach,
            float.1 - reach,
            float.2 + 2 * reach as u32,
            float.3 + 2 * reach as u32,
        );
        let (cw, ch) = (f64::from(canvas.0), f64::from(canvas.1));
        let (left, top, right, bottom) = self
            .map
            .corners(grown)
            .and_then(corner_box)
            .unwrap_or((0.0, 0.0, cw, ch));
        let (left, top) = (left.max(0.0), top.max(0.0));
        let (right, bottom) = (right.min(cw), bottom.min(ch));
        if left >= right || top >= bottom {
            return Ok(None);
        }
        Ok(Some((
            left as i64,
            top as i64,
            (right - left) as u32,
            (bottom - top) as u32,
        )))
    }

    /// [`Self::landing_box`] over the float the edit actually lifted. No work refusal: the
    /// resample runs under the caller's job control.
    pub(crate) fn landing(
        &self,
        float: CanvasRect,
        canvas: (u32, u32),
    ) -> Result<Option<CanvasRect>, String> {
        self.landing_box(float, canvas)
    }

    /// Premultiplied f32 over `float` → premultiplied f32 over `dest`, via sampling::resample_into
    /// (P = [f32; 4], C = 4, report_progress = true, precise_weights = true).
    pub(crate) fn resample(
        &self,
        samples: &[[f32; 4]],
        float: CanvasRect,
        dest: CanvasRect,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        let origin = |x: i64, y: i64| -> Result<(i32, i32), String> {
            Ok((
                i32::try_from(x).map_err(|_| "selected pixels offset overflow")?,
                i32::try_from(y).map_err(|_| "selected pixels offset overflow")?,
            ))
        };
        let map = self
            .map
            .source_map(origin(float.0, float.1)?, origin(dest.0, dest.1)?)?;
        let count = dest.2 as usize * dest.3 as usize;
        let mut out = reserved_vec(count)?;
        out.resize(count, [0.0; 4]);
        resample_into::<[f32; 4], 4>(
            (float.2, float.3),
            (dest.2, dest.3),
            &map,
            self.sampler,
            |i| samples[i],
            |acc, pixel, weight| {
                for k in 0..4 {
                    acc[k] += weight * pixel[k];
                }
            },
            |acc| acc,
            |i, pixel| out[i] = pixel,
            true,
            true,
            job,
        )?;
        Ok(out)
    }
}

const SHAPE: &str = "a selected pixels transform takes matrix, or source_rect with quad";

/// Checks the request's geometry fields before any document is consulted:
/// exactly one of `matrix` or `source_rect` + `quad`, every value finite and
/// a source rect of positive size.
pub(crate) fn check_shape(
    matrix: Option<&[f64; 6]>,
    source_rect: Option<&[f64; 4]>,
    quad: Option<&[f64; 8]>,
) -> Result<(), String> {
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    match (matrix, source_rect, quad) {
        (Some(m), None, None) if finite(m) => Ok(()),
        (None, Some(r), Some(q)) if finite(r) && finite(q) && r[2] > 0.0 && r[3] > 0.0 => Ok(()),
        (Some(_), None, None) | (None, Some(_), Some(_)) => Err(format!(
            "invalid selected pixels request: {SHAPE}, every value finite and a positive source_rect size"
        )),
        _ => Err(format!("invalid selected pixels request: {SHAPE}")),
    }
}

/// [r·a·c, g·a·c, b·a·c, a·c]
pub(crate) fn float_sample(straight: [f32; 4], c: f32) -> [f32; 4] {
    let a = straight[3] * c;
    [straight[0] * a, straight[1] * a, straight[2] * a, a]
}

/// Unpremultiply, then clamp (sampling::straight_unorm's order). Some((rgba, 1.0)) is moved_pixel's `float`.
/// None when a ≤ 0 or a is NaN.
pub(crate) fn landing_sample(premultiplied: [f32; 4]) -> Option<([f32; 4], f32)> {
    let a = premultiplied[3];
    if a.is_nan() || a <= 0.0 {
        return None;
    }
    let unit = |v: f32| (v / a).clamp(0.0, 1.0);
    Some((
        [
            unit(premultiplied[0]),
            unit(premultiplied[1]),
            unit(premultiplied[2]),
            a.clamp(0.0, 1.0),
        ],
        1.0,
    ))
}

/// The 32-bit float twin of [`landing_sample`]: `sampling::straight_extended`, the master
/// copy, unpremultiplies by the raw alpha and clamps RGB to `range`, the float's own visible range
/// (D3), dropping alpha below `EXTENDED_ALPHA_FLOOR` to transparent. None when that leaves alpha 0.
pub(crate) fn landing_extended(
    premultiplied: [f32; 4],
    range: &crate::sampling::ColorRange,
) -> Option<([f32; 4], f32)> {
    let straight = crate::sampling::straight_extended(premultiplied, range);
    (straight[3] > 0.0).then_some((straight, 1.0))
}
