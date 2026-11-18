//! KERNEL: the Layer Style Outside Stroke's distance MEASURE (#2) — how far
//! an outside pixel is from the shape, and the outside band built from it —
//! and the Shape Burst's contour distance ("The contour distance").
//!
//! # The measure
//!
//! A pixel's distance to the shape is the two-pass 5 × 5 CHAMFER distance
//! between pixel centres, with the Euclidean lengths of its three steps as
//! weights: 1 for an axial step, √2 for a diagonal, √5 for a knight's move
//! (`WEIGHTS`), SEEDED from every pixel the shape touches: a pixel of
//! coverage `c ≥ 1` starts at `1 − c / 255` (`seed`), so a fully covered
//! pixel is distance 0 and an anti-aliased one starts as far inside the
//! band as its uncovered share. The band then grows as
//! `style_plane_ops::outside_stroke_band` does — `grown =
//! coverage_byte(size + 0.5 − D)` — knocked out by `255 − coverage`.
//!
//! # Why a chamfer, and why these weights
//!
//! Photoshop's corner arc is NOT a function of the Euclidean distance: in
//! the `stacked-effects-557` take2 7 px stroke, the pixels at offsets (1, 7)
//! and (5, 5) from the rect's corner pixel are both √50 = 7.071 away, yet
//! Photoshop gives them coverage 0.765 and 0.933. The 5 × 5 chamfer gives
//! (1, 7) the path √5 + 5 = 7.236 and (5, 5) the path 5√2 = 7.071, i.e.
//! `size + 1 − D` = 0.764 and 0.929 — and it reproduces every measured
//! hard corner at 1–10 px within about one code
//! (`scripts/photoshop-probe/stroke_exposure_578_take.py`, take1). Along a
//! straight edge every step is axial, so the chamfer distance equals the
//! Euclidean one and the band's straight-edge rule is unchanged.
//!
//! # Why these seeds
//!
//! A shape with an anti-aliased edge (a disc, a feathered rect, a layer at
//! partial alpha) has no "inside" threshold Photoshop honours: its band is
//! measured from the coverage-weighted edge. Seeded at `1 − c / 255` from
//! every touched pixel, the band reproduces take2 of
//! `scripts/photoshop-probe/stroke_exposure_578_take2.py` (receipt
//! `~/devel/doctored-fixtures/photoshop/stroke-exposure-578/take2`) where
//! the old seeds (coverage ≥ 128 at 0) missed by up to 192 codes: of the 20
//! seed × knock-out members declared before the take, this one (A1K0)
//! ranked first, and with Photoshop's own composite it fits every disc,
//! soft-edge, half-alpha and 0 %-fill row within 1.02 codes. A
//! hard-edged shape (coverage 0 or 255) seeds exactly as before, so every
//! hard corner and straight edge is byte-identical (the take's GUARD).
//!
//! # The band under the layer (U1)
//!
//! Photoshop joins an outside band and the layer over it as a DISJOINT
//! UNION: under a pixel the layer lands on at `a = c · f` (coverage times
//! Fill), the band takes `b` of the pixel and the layer `a`, so the result
//! is `a·S + b·R + (1 − a − b)·W`. Drawing the band at `b` and the layer
//! `over` it gives `a·S + (1 − a)·b·R + …` instead: the band's share shrinks
//! by `1 − a` a second time, which left the anti-aliased rim 57–64 codes off
//! at 100 % fill. [`band_under_layer`] lands the band at `b / (1 − a)`, so
//! the layer's `over` brings it back to exactly `b`; where `b > 1 − a` the
//! two shares overlap, and the clamp at 1 lets the band fill what the layer
//! leaves. `scripts/photoshop-probe/stroke_composite_578_take.py` fitted it
//! offline (1.02 codes on all 20 anti-aliased Outside rows of take1 and
//! take2, against 64) and its take (`stroke-composite-578/take1`) held it
//! within about one code on every held-out row, Fill 30 and 50 % and stroke
//! opacity 60 % included. A hard shape (`a` 0 or 1) and a layer at 0 % Fill
//! divide by 1, so they stay byte-identical.
//!
//! # The inside band on an anti-aliased shape (NA1)
//!
//! [`inside_corner_band`] measures Center's inner half, and the Inside band
//! of a shape with a partial pixel, from the
//! anti-aliased edge, the outer half's seeds turned inside out: the same
//! chamfer over the INVERTED plane, so a pixel of coverage `c ≤ 254` seeds
//! at `c / 255`, and the band is `c − coverage_byte(D − size − 0.5)`. The
//! old inner half thresholds the shape at 50 % (`inside_stroke_band`) and
//! left a disc's Center row 99 codes off. Through Doctored's composite this
//! one puts the take's disc Center 6 within one code (from 115) and cuts disc
//! Center 3 at 50 % Fill from 80 to 43, the soft Center 4 at 50 % from 69 to
//! 42 and take2's disc Center 4 at 50 % from 99 to 63, without worsening any
//! row; what remained was the composite ("Center's two halves"). On a hard
//! rect it is byte-identical to the old band at every half size 0.5…5 (the
//! runner's GUARD). On a hard shape that is not a rect (a diagonal or curved edge of
//! coverage 0 and 255) the two measures differ wherever the nearest
//! transparent pixel lies off-axis (the chamfer's √5 + 1 = 3.236 against the
//! exact √10 = 3.162), so a Center stroke there moves; no take measured it.
//!
//! An Inside stroke on a shape with a partial pixel takes the same band
//! (#2; `style_fx_stroke::inner_band`), under the unchanged `o · c`
//! replacement; a binary shape keeps `inside_stroke_band`. The fourth Stroke
//! take (`stroke_aa_578_take4.py`) picked it after its result (the diagnostic
//! `Sn`: every take4 Inside row within 1.02 codes), so it shipped only on the
//! confirm take (`stroke_aa_578_confirm5.py`, stimuli it had not seen: discs
//! of r 17.5 and 30, a rounded rect, a half plane at coverage 0.125 – 0.875,
//! Size 2 – 12, Fill and stroke opacity 100 and 50 %, 8 and 16 bits): all 160
//! rows within 1.02 / 0.25 codes, SHIP by rule (i), where the
//! anti-aliased-contour band it replaced (`style_aa_band`) kept up to 10.2 /
//! 0.93 at the band's inner edge on the discs. take 3's disc Inside 3 falls
//! from 16 codes to 1.0 (from 115 before the fourth take).
//!
//! # Center's two halves
//!
//! A Center stroke's outer half lands in the Below group, so B0, the stack
//! its inner half replaces over, already holds the outer half under a
//! partial pixel. Landing the inner half's ink there at `c · o` stacked the
//! two halves (at 50 % opacity 0.53 – 0.75 of the pixel where the stroke
//! holds 0.5) and, below 100 % Fill, left part of the stack beneath where
//! the stroke fills the pixel. [`inner_half_alpha`] lands it at the
//! alpha that COMPLETES the stroke's share over the outer half's landed
//! `ba`, `(min(1, (c + outer) · o) − ba) / (1 − ba)`, so the halves and the
//! layer join as one disjoint union, the outside band's rule
//! ([`band_under_layer`]) carried across the edge: the stroke holds
//! `(outer + inner) · o` of the pixel, the layer `(c − inner) · f`, the stack
//! the rest. Where the outer half has no coverage (every Inside stroke,
//! every hard shape) the ink keeps `c · o`, and at `f = o` the two
//! composites are one.
//!
//! A recorded take measured the match. Judged first on the rows already on
//! disk (the fourth Stroke take's Center rows and takes 1 – 3's: every Fill
//! 50 and 50 %-opacity row of Size 3 and up from a max of 17 – 63 codes to
//! 1.02, the 8-bit 50 %-opacity rows keeping the opacity's byte in their
//! mean), it was confirmed on `stroke_center_2_take.py`'s take1 (Photoshop
//! 26.10.0; 288 rows on four pictures no earlier take showed: a half plane
//! at coverage 0.2 – 0.8, a three-column soft edge, an r 23.25 disc and a
//! rounded rect; Size 1 – 10, Fill 100 / 50 / 0, stroke opacity 100 / 50, 8
//! and 16 bits): every row of Size 2 and up within 1.02 / 0.155 codes
//! against the bound of 2 / 0.5 declared before it, where the stacked
//! halves missed by up to 64. Size 1 (`n = 0.5`) does not meet it: there the
//! two halves leave part of a partial pixel, and the take keeps the layer at
//! `c · f · (1 − t)`, `t` the halves' coverage with the outside band knocked
//! out as `grown − c`. That form, declared before the take, read all 288
//! rows within the same 1.02 / 0.155; it is not built because over B0 it
//! cannot be reached where the outer half lands at 1 under the layer (B0
//! then holds no trace of the stack beneath), so it waits on keeping that
//! stack beside B0 (#2).
//!
//! # The contour distance
//!
//! [`contour_distance`] is the Shape Burst's `D` (`style_stroke_fill`): the
//! 8-neighbour chamfer (axial 1, diagonal √2, no knight's move) from every
//! pixel to the nearest pixel on the OTHER side of the coverage ≥ 128
//! contour, so a pixel next to the contour reads 1. It is not the band's
//! measure on purpose: the band's coverage is cut by the seeded 5 × 5
//! chamfer above (the measured corner arc, #2), but the ramp ACROSS the
//! band follows the 3 × 3 one. In take 2 of
//! `scripts/photoshop-probe/gradient_identify_673_take2.py` the Smoothness 0
//! burst is linear in this `D` with no offset (divisor Size + 1) on the
//! rect, the disc and the anti-aliased disc alike, and on the disc's inside
//! row the 3 × 3 distance fits exactly where the Euclidean one is 1.6 / 0.49
//! codes off (Size 16). Both measures run through the ONE relax loop
//! (`relax_passes`), the 3 × 3 mask being the 5 × 5 one's first four steps.
//!
//! # Cost
//!
//! One `f32` plane, two raster passes of eight neighbours each, polling the
//! job once per row of each pass — less scratch than the exact EDT's two
//! planes it replaces here. Edit ▸ Stroke… keeps `outside_stroke_band` (no
//! Photoshop evidence for its corners), and `grow_mask` /
//! `dilate_plane_with_job` are untouched: the drop shadow's spread and the
//! outer glow use them.

use crate::doc_select::coverage_byte;
use crate::job_control::JobContext;
use crate::style_plane_ops::multiply_planes;

/// The chamfer's step weights: axial 1, diagonal √2, knight √5 (module
/// doc). √5 has no `std` constant; 2.236_068 is its nearest `f32`.
const WEIGHTS: (f32, f32, f32) = (1.0, std::f32::consts::SQRT_2, 2.236_068);

/// The forward pass's eight neighbours, `(dx, dy, weight index)`: every
/// neighbour of the 5 × 5 mask already visited in raster order (the row
/// above, two rows above for a knight, and the pixel to the left). The
/// backward pass mirrors them.
const FORWARD: [(isize, isize, usize); 8] = [
    (-1, 0, 0),
    (-1, -1, 1),
    (0, -1, 0),
    (1, -1, 1),
    (-1, -2, 2),
    (1, -2, 2),
    (-2, -1, 2),
    (2, -1, 2),
];

/// A pixel's starting distance (module doc, "Why these seeds"): `1 − c / 255`
/// for a touched pixel (0 when fully covered), `+inf` for an untouched one.
fn seed(c: u8) -> f32 {
    if c == 0 {
        f32::INFINITY
    } else {
        1.0 - f32::from(c) / 255.0
    }
}

/// The seeded 5 × 5 chamfer distance (module doc) from every pixel centre to
/// the shape: the least `seed(q) + path(q, p)` over the touched pixels `q`;
/// 0 at a fully covered pixel, `+inf` everywhere when the plane touches no
/// pixel. `coverage` is `w × h`. Polls `job` once per row of each pass.
pub(crate) fn chamfer_distance_to_inside(
    coverage: &[u8],
    w: usize,
    h: usize,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    job.check()?;
    let mut d: Vec<f32> = coverage.iter().map(|&v| seed(v)).collect();
    relax_passes(&mut d, w, h, &FORWARD, job)?;
    Ok(d)
}

/// The 8-neighbour chamfer (axial 1, diagonal √2) from every pixel to the
/// nearest pixel across the coverage ≥ 128 contour (module doc, "The
/// contour distance"): with `to_inside`, an outside pixel (coverage < 128)
/// measures to the inside and an inside pixel reads 0; with `!to_inside`
/// the reverse. `+inf` everywhere when the other side is empty. `coverage`
/// is `w × h`. Polls `job` once per row of each pass.
pub(crate) fn contour_distance(
    coverage: &[u8],
    w: usize,
    h: usize,
    to_inside: bool,
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    job.check()?;
    let mut d: Vec<f32> = coverage
        .iter()
        .map(|&v| {
            if (v >= 128) == to_inside {
                0.0
            } else {
                f32::INFINITY
            }
        })
        .collect();
    // The 3 × 3 mask is the 5 × 5 one without its knight's moves: the
    // first four of `FORWARD`.
    relax_passes(&mut d, w, h, &FORWARD[..4], job)?;
    Ok(d)
}

/// The ONE two-pass chamfer relax loop, over the forward half `mask` of a
/// chamfer mask (`(dx, dy, weight index)` into `WEIGHTS`; the backward pass
/// mirrors it): every pixel of `d` (`w × h`) lowered to its least neighbour
/// plus the step. Polls `job` once per row of each pass.
fn relax_passes(
    d: &mut [f32],
    w: usize,
    h: usize,
    mask: &[(isize, isize, usize)],
    job: JobContext<'_>,
) -> Result<(), String> {
    let weights = [WEIGHTS.0, WEIGHTS.1, WEIGHTS.2];
    let relax = |d: &mut [f32], x: usize, y: usize, sign: isize| {
        let i = y * w + x;
        let mut best = d[i];
        for &(dx, dy, k) in mask {
            let (qx, qy) = (x as isize + sign * dx, y as isize + sign * dy);
            if qx < 0 || qy < 0 || qx >= w as isize || qy >= h as isize {
                continue;
            }
            let candidate = d[qy as usize * w + qx as usize] + weights[k];
            if candidate < best {
                best = candidate;
            }
        }
        d[i] = best;
    };
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            relax(d, x, y, 1);
        }
    }
    for y in (0..h).rev() {
        job.check()?;
        for x in (0..w).rev() {
            relax(d, x, y, -1);
        }
    }
    Ok(())
}

/// The Layer Style's outside stroke band at `size` px: `grown =
/// coverage_byte(size + 0.5 − D)` with `D` the seeded chamfer distance (255
/// on a fully covered pixel), knocked out by the shape (`× (255 −
/// coverage)`) — the steps of `style_plane_ops::outside_stroke_band` with
/// the measure and the seed changed (module doc). A non-positive or
/// non-finite `size`, or a plane that is not `w× h`, grows nothing, as `dilate_plane_with_job` does there.
pub(crate) fn outside_corner_band(
    coverage: &[u8],
    w: u32,
    h: u32,
    size: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    job.check()?;
    let (wu, hu) = (w as usize, h as usize);
    let mut grown = if !size.is_finite() || size <= 0.0 || coverage.len() != wu * hu {
        coverage.to_vec()
    } else {
        chamfer_distance_to_inside(coverage, wu, hu, job)?
            .into_iter()
            .map(|d| coverage_byte(size + 0.5 - d))
            .collect()
    };
    job.check()?;
    let inverted: Vec<u8> = coverage.iter().map(|&v| 255 - v).collect();
    job.check()?;
    multiply_planes(&mut grown, &inverted);
    Ok(grown)
}

/// The alpha a Below stroke band of alpha `sa` lands with under a pixel the
/// layer covers at `layer_alpha` (module doc, "The band under the layer"):
/// `min(1, sa / (1 − layer_alpha))`, so the layer's `over` leaves the band
/// exactly `sa` of the pixel. `sa` unchanged where `layer_alpha` is not in
/// (0, 1): nothing is under the band, or the band is knocked out there.
pub(crate) fn band_under_layer(sa: f32, layer_alpha: f32) -> f32 {
    if !(layer_alpha > 0.0 && layer_alpha < 1.0) {
        return sa;
    }
    (sa / (1.0 - layer_alpha)).min(1.0)
}

/// The alpha a Center stroke's inner half lands its ink with over B0 at a
/// pixel its outer half also covers (module doc, "Center's two halves"):
/// `shape_alpha` the pixel's shape coverage times the stroke's opacity,
/// `outer_alpha` the outer half's coverage there times the opacity (before
/// landing), `landed` the alpha the outer half landed with
/// (`band_under_layer`). The ink COMPLETES the stroke's share of the pixel,
/// `(min(1, shape_alpha + outer_alpha) − landed) / (1 − landed)`, so over B0
/// (which already holds `landed`) the stroke holds `shape_alpha +
/// outer_alpha` of it, never the two halves stacked; clamped to [0, 1], and
/// 0 where the outer half already fills the pixel (`landed ≥ 1`).
pub(crate) fn inner_half_alpha(shape_alpha: f32, outer_alpha: f32, landed: f32) -> f32 {
    if landed < 1.0 {
        (((shape_alpha + outer_alpha).min(1.0) - landed) / (1.0 - landed)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The inside band at `size` px measured from the anti-aliased edge (module
/// doc, "The inside band on an anti-aliased shape"), for both callers in
/// `style_fx_stroke::inner_band`: Center's inner half (`size` already halved
/// by the caller) and the Inside band of a shape with a partial pixel: `D` the
/// seeded chamfer over the inverted plane (`chamfer_distance_to_inside`, so a
/// pixel of coverage `c ≤ 254` seeds at `c / 255`), the band `max(c −
/// coverage_byte(D − size − 0.5), 0)`. A non-positive or non-finite `size`,
/// or a plane that is not `w × h`, is no band.
pub(crate) fn inside_corner_band(
    coverage: &[u8],
    w: u32,
    h: u32,
    size: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    job.check()?;
    let (wu, hu) = (w as usize, h as usize);
    if !size.is_finite() || size <= 0.0 || coverage.len() != wu * hu {
        return Ok(vec![0; coverage.len()]);
    }
    let inverted: Vec<u8> = coverage.iter().map(|&v| 255 - v).collect();
    let d = chamfer_distance_to_inside(&inverted, wu, hu, job)?;
    job.check()?;
    Ok(coverage
        .iter()
        .zip(d)
        .map(|(&c, d)| c.saturating_sub(coverage_byte(d - size - 0.5)))
        .collect())
}
