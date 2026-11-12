//! Version-one ordered native brush footprints. Binary64 dab geometry is
//! separate from the bounded request envelope; source-over coverage is rounded
//! only after the complete stroke. Soft ellipses share RAW-local smoothstep.
//!
//! Since 13A-6c-1 the binary64 field spans the stroke's FOOTPRINT `F` — the
//! union of the clipped dab boxes of every dab with nonzero diameter and
//! opacity — instead of the canvas, and a hard dab (hardness 1, not pixel
//! mode, diameter ≥ 1) CLASSIFIES each box pixel before integrating it:
//! INTERIOR pixels take coverage `1.0·d[3]`, EXTERIOR pixels are skipped, and
//! only the BAND between them runs the 8×8 sum. `evaluate` is the ONE per-pixel
//! coverage expression (pixel overlap / soft centre sample / hard 8×8 sum) and
//! `raw_local_coverage::radial` stays the ONE coverage evaluator; the retained
//! canvas-field path `reference_rasterize` (`#[doc(hidden)]`, tests only) calls
//! the same `evaluate` on every box pixel.
//!
//! EXACTNESS. Outside `F` no dab writes, so the reference's field is 0 there
//! and quantizes to 0, which the product writes directly. Inside `F` the
//! accumulation `old += c·(1 − old)` runs per pixel in dab order on both paths,
//! so it suffices that every product coverage `c` equals the reference's bit
//! for bit. Pixel, soft and sub-1-px hard pixels call `evaluate` itself. For a
//! classified hard pixel the four corners are mapped into the normalized frame
//! with `sample`'s own expressions (`cos·dx − sin·dy`, `sin·dx + cos·dy`,
//! divided by `[radius, radius·roundness]`, the distance their `hypot`), so
//! every classifier evaluation and every 8×8 sample carries the same error
//! bound. `radial` at hardness 1 returns 1.0 for computed `d ≤ 1` and 0.0 for
//! computed `d > 1`.
//! - INTERIOR iff all four corners have computed `d ≤ 1 − ε`. The normalized
//!   distance is a norm of an affine image of the pixel, hence convex, so every
//!   sample's TRUE `d` is at most the largest corner's, and its computed `d` is
//!   at most `1 − ε + 2·δ < 1`: all 64 samples return 1.0, the sum is exactly
//!   64.0 and `64.0/64.0·d[3] = 1.0·d[3]` bit for bit.
//! - EXTERIOR iff (a) the dab centre is NOT in the closed pixel square
//!   (comparisons only; the affine map is a bijection, so this is exactly "the
//!   origin lies outside the closed parallelogram P the pixel maps to") AND (b)
//!   the origin's distance to P's four edges (`artwork_coverage::distance`,
//!   min over the segments) is `≥ 1 + ε`. With the origin outside the convex
//!   P, its nearest point of P lies on the boundary, so every point of P — and
//!   so every sample — has true `d ≥ 1 + ε − δ` and computed
//!   `d ≥ 1 + ε − 2·δ > 1`: all samples return 0.0, `c = 0` and `old += 0·(1 − old)` is an
//!   identity on the field, so the write is skipped. Condition (a) is
//!   necessary: a pixel containing a sub-pixel dab or a needle has P around the
//!   origin and every edge far from it.
//! - Otherwise the pixel is BAND and runs `evaluate`.
//!
//! THE MARGIN. `ε = HARD_EDGE_MARGIN = 2^-16`. Over the validated domain —
//! canvas coordinates ≤ 1e8 (`doc::MAX_PIXELS`), and a classified dab has
//! `d[2] ≥ 1`, `roundness ≥ 0.01`, so its minor radius `r·roundness ≥ 0.005`
//! — a mapped component's error is `|Δq| ≤ (√2·2^-27 + (2 + √2)(r + 1)·2^-53)
//! / (r·roundness) < 2.2e-6` (the subtraction `x − d[0]` of values ≤ 1e8, the
//! rotation's two products and sum over `|dx|, |dy| ≤ r + 1`, the division),
//! so the normalized distance carries `δ < 3.1e-6` per evaluation (a
//! `hypot` of two such components) and `2·δ < 6.2e-6 < 2^-16 ≈ 1.53e-5` with
//! headroom. Hard dabs with `0 < d[2] < 1` are NOT classified (the minor
//! radius may fall below 0.005, outside the bound's domain): their clipped box
//! is at most 2×2 and every pixel runs `evaluate`.
//!
//! MONOTONICITY. `plan` charges `n + |F|` up front (F is computed before the
//! dab loop), then per dab: 0 for a zero dab (diameter or opacity 0), `box`
//! for a pixel-mode or soft dab, and `min(64·box, box + 64·band_bound(d[2]))`
//! for a hard one (`box` for the classifier, 64 per band pixel). The
//! pre-13A-6c-1 plan charged `2n` up front and `box·(64 | 1)` for every dab.
//! `|F| ≤ n` and each dab's charge is at most its old one, so at every dab
//! prefix the running total is at most the old running total: nothing the old
//! plan accepted is refused, and both caps and both sentences were unchanged
//! by it. The work cap `MAX_BRUSH_WORK` is now 2·10⁹ (256 million
//! before), so a 5,000 px brush can lay about 70 soft dabs on a 100 MP canvas,
//! and its sentence names the new figure; the 50,000-dab cap is as it was.
//! Scratch is `4·BOOKKEEPING_BYTES + 8·|F| + 2·n + 8·dabs.len()` (the field
//! over F, the u16 output), at most the old `12n` term.
//!
//! THE BAND BOUND. Let C be the dab's ellipse (semi-major `a = d[2]/2`) and
//! P a band pixel of a classified dab. P is not INTERIOR, so some corner lies
//! outside the (1 − ε)-ellipse; P is not EXTERIOR, so either P contains the
//! centre (while some corner lies outside the (1 − ε)-ellipse) or some edge
//! comes within normalized distance 1 + ε of the origin, i.e. P meets the
//! closed (1 + ε)-ellipse. Either way, P being connected, it meets the annulus
//! between the (1 − ε)- and (1 + ε)-ellipses, which lies inside `C ⊕ disc(ε·a)`
//! and outside `C ⊖ disc(ε·a)` (support functions: `(1 ± ε)·h_C` is within
//! `ε·a` of `h_C` because `h_C ≤ a`). A unit cell has diameter √2, so every
//! cell meeting that annulus lies in `(C ⊕ disc(ρ)) \ (C ⊖ disc(ρ))`,
//! `ρ = √2 + ε·a`, and the cells are disjoint: their count is at most
//! `area(C ⊕ disc ρ) − area(C ⊖ disc ρ) ≤ (A + Pρ + πρ²) − (A − Pρ) = 2Pρ + πρ²`
//! (Steiner for the dilation; `A − Pρ` bounds the erosion of a convex set),
//! with the perimeter `P ≤ 2πa`: `band_bound(d) = ceil(4πaρ + πρ²)`. The
//! computed tests move the annulus by `δ` (above), far inside the counting
//! slack; the suite pins `band_count ≤ band_bound`. For `d[2] < 1` the box is
//! at most 4 cells and `band_bound ≥ ceil(2π) = 7`.
//!
//! Soft ellipses share RAW-local smoothstep; hard ellipses declare an 8×8 area approximation, pixel
//! mode exact square overlap. Ordered coverage accumulates continuously, optional global opacity
//! precedes its sole u16 round, and caller output stages atomically. Legacy byte and unversioned
//! request arithmetic remains unchanged. Since 13A-6c-1 `brush_coverage::rasterize` accumulates its
//! f64 field over the stroke's FOOTPRINT (the union of the nonzero dabs' clipped boxes) and
//! classifies each pixel of a hard dab of diameter ≥ 1 exactly — INTERIOR (all four corners within
//! the `1 − HARD_EDGE_MARGIN` ellipse) takes `1.0·d[3]`, EXTERIOR (centre outside the closed pixel
//! AND every parallelogram edge at normalized distance ≥ `1 + HARD_EDGE_MARGIN`, via
//! `artwork_coverage::distance`) is skipped — so only the edge BAND runs the 8×8 sum through
//! `evaluate`, the ONE per-pixel expression; `plan` charges `n + |F|` plus `min(64·box, box +
//! 64·band_bound)` per hard dab, never above the pre-13A-6c-1 figure. The `#[doc(hidden)]`
//! canvas-field path `reference_rasterize` (with `band_count` / `band_bound` / `footprint`) is the
//! `tests/bounded_brush_footprint.rs` oracle; this module doc is the exactness, margin,
//! monotonicity and band-bound argument.
//!
//! THE ALIASED TIP (the Pencil). A request with `aliased: true` (hardness 1 only) cuts each
//! pixel's coverage, before the dab's opacity, to 1 where `evaluate` gives at least one half
//! and to 0 below: the 8×8 sum's 32 of 64 samples, pixel mode's half square. The cut lives in
//! `evaluate`, so the product, the reference and `dab_coverage` share it, and it agrees with the
//! classifier (INTERIOR's 1 and EXTERIOR's 0 are fixed points of the cut), so the exactness
//! argument above holds unchanged. A half is the rule because it keeps a dab's painted area the
//! hard dab's area to within its band: a pixel more than half inside is painted, one more than
//! half outside is not. The dab's opacity, the source-over accumulation and the one quantization
//! are untouched, so a deposit-1 aliased stroke is two-valued (0 or the request's opacity) and
//! a pressure-opacity one accumulates as any other. `plan` is unchanged: the cut costs nothing
//! beyond the band sum it already charges. Absent, the key keeps every earlier request's bytes.
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use serde::Deserialize;

pub const MAX_BRUSH_DABS: usize = 50_000;
/// The work one native brush job may plan: 2·10⁹, the figure of the
/// interactive budget (`precision_render::MAX_WORK_UNITS`), about 70 soft
/// 5,000 px dabs on a 100 MP canvas. It was 256 million before.
pub const MAX_BRUSH_WORK: u64 = 2_000_000_000;
/// The work one Smudge stroke may plan: the figure `MAX_BRUSH_WORK`
/// had before it was raised. Smudge walks its stroke a pixel at a time
/// (`retouch_smudge`: one whole dab per sub-step), and the canvas runs the
/// 8-bit stroke synchronously on its final tick, so it keeps the bound that
/// was sized for a synchronous stroke rather than the brush job's. With the
/// tool's largest tip, 200 px, that is about 6,270 px of travel soft
/// (40,805 units a sub-step) and 1,650 px hard (155,109).
pub const MAX_SMUDGE_WORK: u64 = 256_000_000;
/// [`MAX_BRUSH_WORK`]'s refusal. Kept: a stroke is gesture input the
/// canvas rasterizes on every tick, so the sentence says what to do instead.
const WORK_REFUSAL: &str =
    "native brush exceeds 2 billion footprint work units; use a shorter stroke or a smaller brush";
/// Normalized-d classification margin, 2^-16. Covers TWICE the f64 error bound of the
/// normalized distance over the validated domain — canvas coordinates <= 1e8 (doc::MAX_PIXELS),
/// minor radius radius*roundness >= 0.005 (classified dabs have d[2] >= 1, roundness >= 0.01):
/// |Δq| per component <= (√2·2^-27 + (2+√2)(r+1)·2^-53) / (r·roundness) < 2.2e-6, so
/// |Δd| < 3.1e-6 per evaluation and 2·3.1e-6 < 2^-16 with headroom (module doc states this).
pub const HARD_EDGE_MARGIN: f64 = 1.52587890625e-5;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushRequest {
    version: u32,
    hardness: f64,
    roundness: f64,
    angle_degrees: f64,
    pixel: bool,
    #[serde(default = "one")]
    opacity: f64,
    /// The Pencil's tip: coverage cut at one half (module doc, THE ALIASED TIP).
    #[serde(default)]
    aliased: bool,
}
pub struct BrushPlan {
    pub scratch_bytes: u64,
    pub output_samples: usize,
    pub work_units: u64,
}
/// One dab's geometry in the frame `sample` evaluates.
struct Tip<'a> {
    d: &'a [f64],
    radius: f64,
    minor: f64,
    sin: f64,
    cos: f64,
}
/// A classified hard pixel.
#[derive(PartialEq)]
enum Class {
    Interior,
    Exterior,
    Band,
}
impl Tip<'_> {
    /// SoftBrush's positive tip angle is counterclockwise on screen. In y-down
    /// canvas coordinates its inverse rotates by +angle.
    fn frame(&self, x: f64, y: f64) -> [f64; 2] {
        let (dx, dy) = (x - self.d[0], y - self.d[1]);
        [self.cos * dx - self.sin * dy, self.sin * dx + self.cos * dy]
    }
    fn sample(&self, x: f64, y: f64, hardness: f64) -> f64 {
        crate::raw_local_coverage::radial(
            self.frame(x, y),
            [0.; 2],
            [self.radius, self.minor],
            hardness,
        )
    }
    /// The frame point `radial` divides: its `hypot` is `radial`'s `d`.
    fn normalized(&self, x: f64, y: f64) -> [f64; 2] {
        let p = self.frame(x, y);
        [p[0] / self.radius, p[1] / self.minor]
    }
    /// The module doc's classification of pixel `(x, y)` of a hard dab with
    /// `d[2] >= 1`.
    fn classify(&self, x: usize, y: usize) -> Class {
        let (left, top) = (x as f64, y as f64);
        let (right, bottom) = (left + 1., top + 1.);
        let q = [
            self.normalized(left, top),
            self.normalized(right, top),
            self.normalized(right, bottom),
            self.normalized(left, bottom),
        ];
        if q.iter().all(|p| p[0].hypot(p[1]) <= 1. - HARD_EDGE_MARGIN) {
            return Class::Interior;
        }
        let d = self.d;
        let contains = left <= d[0] && d[0] <= right && top <= d[1] && d[1] <= bottom;
        if !contains
            && (0..4)
                .map(|i| crate::artwork_coverage::distance([0.; 2], q[i], q[(i + 1) % 4]))
                .fold(f64::INFINITY, f64::min)
                >= 1. + HARD_EDGE_MARGIN
        {
            Class::Exterior
        } else {
            Class::Band
        }
    }
}
impl BrushRequest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let request: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid native brush request: {e}"))?;
        if request.version != 1
            || !unit(request.hardness)
            || !unit(request.opacity)
            || !request.roundness.is_finite()
            || !(0.01..=1.).contains(&request.roundness)
            || !request.angle_degrees.is_finite()
            || !(-180. ..=180.).contains(&request.angle_degrees)
        {
            return Err("native brush requires version1 and normalized valid geometry".into());
        }
        if request.aliased && request.hardness < 1. {
            return Err("native brush aliased tip requires hardness 1".into());
        }
        Ok(request)
    }
    pub fn plan(&self, dimensions: (u32, u32), dabs: &[f64]) -> Result<BrushPlan, String> {
        let count = pixel_count(dimensions.0, dimensions.1)?;
        // The ordering guard: refuse an over-long or ragged list before `footprint` walks it
        // (`validate_dabs` below repeats the check).
        check_count(dabs)?;
        let field = footprint(dimensions, dabs).map_or(0, area);
        let mut work = count as u64 + field;
        validate_dabs(dabs, |d| {
            work += self.dab_work(dimensions, d);
            if work > MAX_BRUSH_WORK {
                return Err(WORK_REFUSAL.into());
            }
            Ok(())
        })?;
        if work > MAX_BRUSH_WORK {
            return Err(WORK_REFUSAL.into());
        }
        Ok(BrushPlan {
            scratch_bytes: 4 * BOOKKEEPING_BYTES
                + field * 8
                + count as u64 * 2
                + dabs.len() as u64 * 8,
            output_samples: count,
            work_units: work,
        })
    }
    pub(crate) fn rasterize(
        &self,
        dimensions: (u32, u32),
        dabs: &[f64],
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let plan = self.plan(dimensions, dabs)?;
        let [fx0, fy0, fx1, fy1] = footprint(dimensions, dabs).unwrap_or([0; 4]);
        let fw = fx1 - fx0;
        let samples = fw * (fy1 - fy0);
        let mut field = reserved_vec(samples)?;
        for i in 0..samples {
            job.every(i)?;
            field.push(0_f64);
        }
        let (sin, cos) = self.angle_degrees.to_radians().sin_cos();
        for (di, d) in dabs.chunks_exact(4).enumerate() {
            job.check()?;
            if d[2] == 0. || d[3] == 0. {
                continue;
            }
            let [x0, y0, x1, y1] = bounds(d, dimensions);
            let tip = self.tip(d, sin, cos);
            let classified = self.classifies() && d[2] >= 1.;
            for y in y0..y1 {
                job.check()?;
                for x in x0..x1 {
                    job.every(x)?;
                    let coverage = if !classified {
                        self.evaluate(&tip, x, y)
                    } else {
                        match tip.classify(x, y) {
                            Class::Interior => 1.,
                            Class::Exterior => continue,
                            Class::Band => self.evaluate(&tip, x, y),
                        }
                    } * d[3];
                    let old = &mut field[(y - fy0) * fw + (x - fx0)];
                    *old += coverage * (1. - *old);
                }
            }
            job.report((di + 1) as f64 / (dabs.len() / 4).max(1) as f64 * 0.9, 3)?;
        }
        let (width, height) = (dimensions.0 as usize, dimensions.1 as usize);
        let mut output = reserved_vec(plan.output_samples)?;
        let mut i = 0;
        for y in 0..height {
            for x in 0..width {
                job.every(i)?;
                i += 1;
                let value = if (fx0..fx1).contains(&x) && (fy0..fy1).contains(&y) {
                    field[(y - fy0) * fw + (x - fx0)]
                } else {
                    0.
                };
                output.push(self.quantize(value));
            }
        }
        job.check()?;
        Ok(output)
    }
    /// The pre-13A-6c-1 path: canvas-sized f64 field, every dab's box, 8x8 on every hard pixel,
    /// same accumulation and quantization, JobContext::NONE, geometry validated, NO work cap (tests only).
    /// Differs from the pre-13A-6c-1 body only by calling the shared per-pixel evaluator (Approach 1).
    #[doc(hidden)]
    pub fn reference_rasterize(
        &self,
        dimensions: (u32, u32),
        dabs: &[f64],
    ) -> Result<Vec<u16>, String> {
        let count = pixel_count(dimensions.0, dimensions.1)?;
        validate_dabs(dabs, |_| Ok(()))?;
        let job = JobContext::NONE;
        let mut field = reserved_vec(count)?;
        for i in 0..count {
            job.every(i)?;
            field.push(0_f64);
        }
        let (sin, cos) = self.angle_degrees.to_radians().sin_cos();
        for (di, d) in dabs.chunks_exact(4).enumerate() {
            job.check()?;
            if d[2] == 0. || d[3] == 0. {
                continue;
            }
            let [x0, y0, x1, y1] = bounds(d, dimensions);
            let tip = self.tip(d, sin, cos);
            for y in y0..y1 {
                job.check()?;
                for x in x0..x1 {
                    job.every(x)?;
                    let coverage = self.evaluate(&tip, x, y) * d[3];
                    let old = &mut field[y * dimensions.0 as usize + x];
                    *old += coverage * (1. - *old);
                }
            }
            job.report((di + 1) as f64 / (dabs.len() / 4).max(1) as f64 * 0.9, 3)?;
        }
        let mut output = reserved_vec(count)?;
        for (i, value) in field.into_iter().enumerate() {
            job.every(i)?;
            output.push(self.quantize(value));
        }
        job.check()?;
        Ok(output)
    }
    /// Pixels of `dab`'s clipped box the classifier sends to the 8x8 loop (0 for pixel/soft dabs;
    /// the whole clipped box for hard dabs with 0 < d[2] < 1).
    #[doc(hidden)]
    pub fn band_count(&self, dimensions: (u32, u32), dab: &[f64]) -> u64 {
        if dab.len() < 4 || !self.classifies() || dab[2] == 0. || dab[3] == 0. {
            return 0;
        }
        let [x0, y0, x1, y1] = bounds(dab, dimensions);
        if dab[2] < 1. {
            return (x1 - x0) as u64 * (y1 - y0) as u64;
        }
        let (sin, cos) = self.angle_degrees.to_radians().sin_cos();
        let tip = self.tip(dab, sin, cos);
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| tip.classify(x, y) == Class::Band)
            .count() as u64
    }
    /// `(sin, cos)` of the tip angle, as `rasterize` computes it once per
    /// stroke: the pair `dab_coverage` takes.
    pub(crate) fn rotation(&self) -> (f64, f64) {
        self.angle_degrees.to_radians().sin_cos()
    }
    /// One dab's coverage of pixel `(x, y)`, before the dab's opacity, exactly
    /// as `rasterize` computes it: a hard dab of diameter ≥ 1 is classified
    /// (1 inside, 0 outside, the 8×8 sum on the band), every other dab runs
    /// `evaluate`. For the per-dab walks that cannot use the accumulated
    /// stroke field (`retouch_smudge`).
    pub(crate) fn dab_coverage(
        &self,
        dab: &[f64],
        rotation: (f64, f64),
        x: usize,
        y: usize,
    ) -> f64 {
        let tip = self.tip(dab, rotation.0, rotation.1);
        if self.classifies() && dab[2] >= 1. {
            match tip.classify(x, y) {
                Class::Interior => 1.,
                Class::Exterior => 0.,
                Class::Band => self.evaluate(&tip, x, y),
            }
        } else {
            self.evaluate(&tip, x, y)
        }
    }
    /// `plan`'s charge for one dab (module doc, MONOTONICITY): 0 for a zero
    /// dab, its clipped box for a pixel-mode or soft dab, and `min(64·box,
    /// box + 64·band_bound)` for a hard one.
    pub(crate) fn dab_work(&self, dimensions: (u32, u32), dab: &[f64]) -> u64 {
        if dab[2] == 0. || dab[3] == 0. {
            return 0;
        }
        let [x0, y0, x1, y1] = bounds(dab, dimensions);
        let pixels = (x1 - x0) as u64 * (y1 - y0) as u64;
        if self.classifies() {
            (64 * pixels).min(pixels + 64 * band_bound(dab[2]))
        } else {
            pixels
        }
    }
    /// Hard dabs (hardness 1, not pixel mode) integrate 8x8 and are classified.
    fn classifies(&self) -> bool {
        !self.pixel && self.hardness >= 1.
    }
    fn tip<'a>(&self, d: &'a [f64], sin: f64, cos: f64) -> Tip<'a> {
        let radius = d[2] / 2.;
        Tip {
            d,
            radius,
            minor: radius * self.roundness,
            sin,
            cos,
        }
    }
    /// THE per-pixel coverage expression, before the dab's opacity: the
    /// reference and the product both call it. An aliased tip cuts it at
    /// one half (module doc, THE ALIASED TIP).
    fn evaluate(&self, tip: &Tip<'_>, x: usize, y: usize) -> f64 {
        let coverage = self.continuous(tip, x, y);
        if !self.aliased {
            coverage
        } else if coverage >= 0.5 {
            1.
        } else {
            0.
        }
    }
    fn continuous(&self, tip: &Tip<'_>, x: usize, y: usize) -> f64 {
        let d = tip.d;
        if self.pixel {
            // Pixel mode integrates an axis-aligned square exactly;
            // diameter1 at a pixel center is one full native sample.
            let radius = tip.radius;
            let overlap =
                |p: f64, c: f64| ((p + 1.).min(c + radius) - p.max(c - radius)).clamp(0., 1.);
            overlap(x as f64, d[0]) * overlap(y as f64, d[1])
        } else if self.hardness < 1. {
            tip.sample(x as f64 + 0.5, y as f64 + 0.5, self.hardness)
        } else {
            // Fixed 8x8 area integration is an explicit version1
            // hard-edge approximation, independent of byte brushes.
            let mut sum = 0.;
            for sy in 0..8 {
                for sx in 0..8 {
                    sum += tip.sample(
                        x as f64 + (sx as f64 + 0.5) / 8.,
                        y as f64 + (sy as f64 + 0.5) / 8.,
                        self.hardness,
                    );
                }
            }
            sum / 64.
        }
    }
    fn quantize(&self, value: f64) -> u16 {
        (value.clamp(0., 1.) * self.opacity * 65535.).round() as u16
    }
}
/// The tuple-count cap, then each dab's geometry, calling `each` after each
/// valid dab (so a charge refusal on dab k precedes a geometry refusal on
/// dab k + 1, as before).
fn validate_dabs(
    dabs: &[f64],
    mut each: impl FnMut(&[f64]) -> Result<(), String>,
) -> Result<(), String> {
    check_count(dabs)?;
    for d in dabs.chunks_exact(4) {
        // A finite explicit geometry ceiling prevents huge off-canvas
        // coordinates from saturating integer conversions silently.
        if d.iter().any(|v| !v.is_finite())
            || d[0].abs() > 1_000_000_000.
            || d[1].abs() > 1_000_000_000.
            || !(0. ..=1_000_000.).contains(&d[2])
            || !unit(d[3])
        {
            return Err("native brush dab coordinates, diameter or opacity are invalid".into());
        }
        each(d)?;
    }
    Ok(())
}
fn check_count(dabs: &[f64]) -> Result<(), String> {
    if !dabs.len().is_multiple_of(4) || dabs.len() / 4 > MAX_BRUSH_DABS {
        return Err(
            "native brush exceeds 50000 dabs or has incomplete tuples; use a shorter stroke or wider spacing"
                .into(),
        );
    }
    Ok(())
}
/// ceil(4*pi*a*rho + pi*rho^2), a = diameter/2, rho = sqrt(2) + HARD_EDGE_MARGIN*a (the Steiner bound, module doc).
#[doc(hidden)]
pub fn band_bound(diameter: f64) -> u64 {
    let a = diameter / 2.;
    let rho = std::f64::consts::SQRT_2 + HARD_EDGE_MARGIN * a;
    (4. * std::f64::consts::PI * a * rho + std::f64::consts::PI * rho * rho).ceil() as u64
}
/// [x0, y0, x1, y1) = union of `bounds` over dabs with d[2] > 0 && d[3] > 0; None when that union is empty.
#[doc(hidden)]
pub fn footprint(dimensions: (u32, u32), dabs: &[f64]) -> Option<[usize; 4]> {
    dabs.chunks_exact(4)
        .filter(|d| d[2] > 0. && d[3] > 0.)
        .map(|d| bounds(d, dimensions))
        .filter(|b| b[0] < b[2] && b[1] < b[3])
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
}
fn area(f: [usize; 4]) -> u64 {
    (f[2] - f[0]) as u64 * (f[3] - f[1]) as u64
}
fn unit(v: f64) -> bool {
    v.is_finite() && (0. ..=1.).contains(&v)
}
fn one() -> f64 {
    1.
}
fn bounds(d: &[f64], (w, h): (u32, u32)) -> [usize; 4] {
    let radius = d[2] / 2.;
    [
        (d[0] - radius).floor().clamp(0., f64::from(w)) as usize,
        (d[1] - radius).floor().clamp(0., f64::from(h)) as usize,
        (d[0] + radius).ceil().clamp(0., f64::from(w)) as usize,
        (d[1] + radius).ceil().clamp(0., f64::from(h)) as usize,
    ]
}
