//! PatchMatch inpainting: the ONE nearest-neighbour search, pyramid and vote
//! behind Content-Aware Fill and the Spot Healing Brush.
//!
//! Barnes, Shechtman, Finkelstein & Goldman, *PatchMatch* (SIGGRAPH 2009) for
//! the randomized correspondence search; Wexler, Shechtman & Irani, *Space-
//! Time Completion of Video* (PAMI 2007) for the EM formulation — search a
//! nearest-neighbour field for every patch that overlaps the hole, then vote
//! the hole's colours back from what those patches matched, and repeat.
//!
//! Free functions and plain buffers, no `DrDocument` knowledge: `doc_inpaint`
//! owns the document, the window, the caps and the composition with
//! `poisson`; this module owns the arithmetic. The half of that arithmetic
//! which moves per TARGET rather than per level — the field itself, its
//! propagation and random search, the patch distance and the vote — is
//! `patchmatch_nnf`; every parameter and every reason for one is here.
//!
//! # The parameters, and why each is what it is
//!
//! | parameter | value | why |
//! |---|---|---|
//! | patch side [`PATCH`] | 7 | Barnes' own inpainting value. Odd, so a patch has a centre. Larger keeps more structure and is `O(P²)` slower and needs a wider ring; 5 frees the fill to invent texture that is not in the picture. |
//! | distance | SSD over RGB; integer byte kernel / native f64 | byte differences remain exact integers (`49·3·255² = 9.56e6` fits with room), while native f32 RGB uses f64 differences without a byte search proxy. Distances share f64 storage. With the incumbent passed in for early termination (`patchmatch_nnf`) it is 3–5× cheaper than the naive form. Alpha is NOT in the distance: a source patch containing a pixel too transparent to carry colour (`doc_inpaint`'s 50 % rule) is inadmissible instead. |
//! | PatchMatch iterations | [`FIRST_ITERATIONS`] = 5, then [`WARM_ITERATIONS`] = 2 | the paper's field is largely converged after 4–5 sweeps, most of it in the first two; every later search at the same level starts from the previous field and an image that barely moved, so two suffice. |
//! | random-search ratio | [`SEARCH_ALPHA`] = 1/2 | the paper's value: radii `w, w/2, w/4 … 1`, `floor(log₂ w) + 1` samples. The large radii escape local minima, the small ones do the refinement propagation cannot. |
//! | pyramid factor | 2, at most [`MAX_LEVELS`] = 8 levels | √2 is smoother at twice the levels and buys nothing here. Coarsening stops while the hole is still `2·PATCH` across, the window is down to `4·PATCH`, or the hole's WIDEST margin to the window's edge is down to `2·PATCH` — below that the coarse problem is noise, and the hole must survive to BE the thing the coarsest level solves. It also stops one level before the narrower AXIS loses its last source patch, around a hole at most [`AXIS_STOP_EXTENT`] across, so a hole with one narrow pair of margins is still filled from both axes ([`should_coarsen`]). |
//! | EM iterations | 10 coarsest, more for a wide coarsest hole / 5 middle / 3 finest ([`em_iterations`], [`coarsest_iterations`]) | the coarsest level decides the structure and is nearly free; the finest mostly refines and every iteration there is the expensive one. The finest count is a measured 3, not the customary 2 — see [`em_iterations`]. A single-level run is the coarsest, so it takes 10. A coarsest hole 22 px across or wider takes `short² / 48`, up to `4^(searched levels − 1)`, because 10 does not settle it — see [`coarsest_iterations`]. |
//! | lifted levels | no search, one vote; `lift + 1` levels forced on [`coarsenable`] | a call covering over a megapixel (`doc_inpaint::inpaint_lift` = L > 0) does its last search at level `min(L, depth − 1)`. Each finer level takes the coarser field doubled (`Nnf::new`), measures it once and votes once: real source pixels through an upscaled field, at the cost of one distance and one vote per target instead of 9 to 23 sweeps. The margin test of [`should_coarsen`] is skipped for those L levels, or a hole touching all four edges would search at full resolution. |
//! | vote | similarity-weighted mean, always | see below. |
//!
//! # The vote is a weighted mean at every step — never winner-take-all
//!
//! Every pixel of the hole is covered by up to `P²` target patches, and its
//! new colour is `Σ wᵢ·Sᵢ / Σ wᵢ` with `wᵢ = exp(−Dᵢ / 2σ²)` and `σ²` the 25th
//! percentile of those patches' own distances (Wexler's rule), floored at 1
//! so a perfect match cannot divide by zero. The weights are computed
//! against the smallest distance at that pixel rather than against zero —
//! `exp(−(Dᵢ − D_min)/2σ²)` — which is the same weighted mean up to a common
//! factor and cannot underflow to an all-zero denominator.
//!
//! The tempting last step is winner-take-all on the finest level's final
//! iteration, "for crispness". It is not offered, and the reason is
//! structural: argmin voting hands adjacent hole pixels colours from
//! unrelated parts of the source, and the Poisson step that follows adds a
//! *harmonic* — hence maximally smooth — field, so by construction it cannot
//! remove an interior discontinuity. The crispness would be bought with
//! visible tearing inside the fill that nothing downstream can repair.
//!
//! # Sources: reject, never clamp
//!
//! A source patch origin is admissible only when every one of its `P × P`
//! pixels is valid — not part of any hole, solid enough to carry colour, and
//! inside the sampling ring — tested in `O(1)` per origin against the integral
//! image of the valid plane ([`sources`]). A propagation or random-search candidate that is not
//! admissible is SKIPPED, leaving the incumbent; clamping it to the nearest
//! admissible origin would pile candidates onto the ring's inner edge and
//! draw a visible seam along it.
//!
//! Two counts bound that list. Below [`min_origins`] — `max(256, area/16)`,
//! proportional because 256 is a floor for "can this run at all", not for
//! "will it look like anything" — `doc_inpaint` widens the ring and asks
//! again. Above [`MAX_ORIGINS`] the list is thinned by taking every `k`-th
//! entry, `k = ceil(n / MAX_ORIGINS)`; the stride is stated here because it
//! CHANGES RESULTS: it moves the random initialization, not the search, which
//! may still reach any admissible origin.
//!
//! # The coarsest level's initialization is the onion peel
//!
//! The masked reduction ([`reduce`]) erodes the hole by one coarse pixel a
//! level and the pyramid stops while the hole is still 7–14 px across or
//! wider, so the hole is never erased and something has to put a first colour
//! in it.
//! [`onion_peel`] does: unknown pixels are filled in increasing order of the
//! signed distance field — outermost first — each taking the mean of its
//! already-known 8-neighbours, and a component that reaches no known pixel at
//! all takes the ring's mean colour. The order is a total order on
//! `(distance, y, x)`, so this is deterministic, and since the coarsest level
//! decides the structure of the whole fill, that determinism is the whole
//! result's.
//!
//! # Determinism
//!
//! Every random draw comes from `rng::SplitMix64::at`, addressed by the caller's
//! seed and the position asking for it (level, sweep, target id), so no
//! value depends on how many draws came before it. The SWEEP is counted
//! across the level and not within one `Nnf::search` call: an EM iteration
//! that re-used its predecessor's address would re-test the identical
//! candidate offsets, which is a search that has stopped searching — the bug
//! that made nine of the coarsest level's ten iterations pure propagation.
//! Fixing it measurably improved the fill: over six seeds on a 200 px hole in
//! a structured field, the mean reconstruction error fell from 6.57 to 4.90
//! code values in the middle of the frame and 8.12 to 7.19 at its edge.
//! Distance ties are broken by the lowest `dy`, then `dx`. The same seed
//! therefore gives byte-identical output, which `content_aware_tests`
//! asserts.
//!
//! # Measured cost
//!
//! The search dominates: `targets × iterations × 13 candidates`, each
//! candidate an SSD over at most 147 channels with early termination. On this
//! machine (`--release`), inside `doc_inpaint`'s full driver, a 300 × 300 hole
//! in a 2000 × 1500 document costs 0.53 s of inpainting and a hole at the
//! one-megapixel cap costs 6.1 s; the full table, and the split against the
//! Poisson solve that follows, are in `doc_inpaint`'s module doc, which is
//! where the caps that bound them live.

use crate::inpaint_sample::Sample;
/// Patch side in pixels. Odd, so a patch has a centre.
use crate::job_control::JobContext;

pub(crate) const PATCH: usize = 7;
/// `PATCH²`, the full sample count of an unclipped patch.
pub(crate) const PATCH_AREA: u32 = (PATCH * PATCH) as u32;
/// Hard ceiling on the pyramid depth.
pub(crate) const MAX_LEVELS: usize = 8;
/// Random initialization draws from at most this many admissible origins;
/// beyond it the list is strided (see the module doc).
pub(crate) const MAX_ORIGINS: usize = 1_000_000;

/// One pyramid level's image and masks. Everything is window-sized and in
/// window coordinates.
///
/// `C` is the colour channel count: 3 (RGB, every shipped caller and the
/// default) or 4 (a CMYK document's ink codes, #1). Every statement below
/// is the same arithmetic per channel, so `C = 3` is the RGB code exactly.
pub(crate) struct Plane<S = u8, const C: usize = 3> {
    /// Width in pixels.
    pub(crate) w: usize,
    /// Height in pixels.
    pub(crate) h: usize,
    /// Straight colour, `C` samples per pixel: legacy u8 or native-code f32.
    /// The current estimate, which the vote
    /// rewrites inside the hole and never touches outside it.
    pub(crate) rgb: Vec<S>,
    /// The pixels this fill re-estimates.
    pub(crate) hole: Vec<bool>,
    /// The pixels carrying real colour: not part of ANY hole (another
    /// component's is unknown too) and solid in the sampled image — alpha at
    /// or over the coverage threshold, not necessarily 255, since alpha here
    /// only answers whether there is colour and the RGB beside it is
    /// straight.
    pub(crate) data: Vec<bool>,
    /// Source permission, separate from real boundary data. None preserves the
    /// legacy sampling policy; an explicit mask is AND-reduced at every level.
    pub(crate) eligibility: Option<Vec<bool>>,
}

impl<S: Sample, const C: usize> Plane<S, C> {
    /// The hole's bounding box as `(x0, y0, x1, y1)` inclusive, or `None`
    /// when the hole is empty at this level (the masked reduction can erode
    /// a thin hole away entirely, which is exactly what it is for).
    pub(crate) fn hole_box(&self) -> Option<(usize, usize, usize, usize)> {
        let (mut x0, mut y0) = (usize::MAX, usize::MAX);
        let (mut x1, mut y1) = (0usize, 0usize);
        let mut any = false;
        for (i, inside) in self.hole.iter().enumerate() {
            if !*inside {
                continue;
            }
            let (x, y) = (i % self.w, i / self.w);
            any = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        any.then_some((x0, y0, x1, y1))
    }

    /// Raster-ordered indices of every pixel the mask marks — the write list
    /// the vote takes.
    pub(crate) fn indices(mask: &[bool]) -> Vec<u32> {
        mask.iter()
            .enumerate()
            .filter_map(|(i, m)| m.then_some(i as u32))
            .collect()
    }
}

/// Whether `plane` should be coarsened once more. Four conditions; the third
/// is the one that is easy to get backwards, and the fourth was added later:
///
/// - the hole must still be wider than two patches, or a coarse level cannot
///   see its shape at all;
/// - the window must still be wider than four, or there is no room for a
///   source patch to move;
/// - the WIDEST margin between the hole's box and the window's edge must
///   still be wider than two patches. Halving it leaves one patch, which is
///   the last scale at which a complete source patch fits outside the hole —
///   below it the coarse level has no admissible origin whatever, and a
///   pyramid built past that point simply stops contributing;
/// - and the narrower AXIS must not be about to lose its last source patch
///   around a hole the coarsest level can still settle ("Per axis" below).
///
/// The widest margin, emphatically not the narrowest. The window is the
/// hole's box padded by the ring and CLAMPED to the canvas, so a hole
/// touching an image border has a margin of zero on that side however wide
/// the band on the other three is. Taking the minimum therefore refused to
/// build a pyramid at all for anything within `2·PATCH` of an edge — every
/// corner and frame-edge fill — and, since a single-level run IS the coarsest
/// (see [`em_iterations`]), the finest and most expensive level then ran the
/// 10-iteration coarsest schedule. Measured on a structured texture at the
/// automatic ring: a 300 x 300 hole flush with the left edge of a
/// 1400 x 1400 canvas cost 1.66 s against 0.71 s for the same hole centred,
/// with 2.9x the mean reconstruction error, and the megapixel cap on a
/// 2600 x 2600 canvas cost 19.4 s against 8.5 s. With the widest margin those
/// pairs are 0.69 / 0.71 s and 8.1 / 8.5 s.
///
/// One margin wider than two patches is one band the whole length (or height)
/// of the window in which a complete coarse patch fits, which is exactly what
/// the justification above asks for; a level that turns out to be starved
/// anyway is handled where it is discovered, by
/// `inpaint_em::inpaint_plane` skipping it, because a coarse level is an
/// accelerator and never a requirement.
///
/// # Per axis
///
/// One band is not every side. Take each axis's WIDER margin: `beside`, the
/// wider of left and right, and `above`, the wider of top and bottom. While
/// the narrower of the two holds a complete patch (`PATCH` or more), the
/// hole has sources on both axes. Halve it from `2·PATCH` or less and that
/// axis has none: every source patch of the coarsest level then lies along
/// the other axis, and the coarsest level decides the structure. Measured on
/// a column ramp (a value from `x` alone, 0.1 code a pixel), a 1000 x 1000
/// hole at (300, 150) in 1600 x 1300: on the widest margin alone the pyramid
/// was six levels deep, the bands above and below were 5 and 6 px at the
/// coarsest, every source patch lay beside the hole in the wrong columns,
/// and the fill's mean absolute error was 22.4 codes (largest 44). Stopping
/// one level sooner, with [`coarsest_iterations`], it is 0.000.
///
/// So the pyramid also stops when the narrower axis is at `PATCH..=2·PATCH`,
/// but only around a hole whose short side is at most [`AXIS_STOP_EXTENT`]
/// at that level. Stopping sooner hands the coarsest level a wider hole,
/// which costs it iterations by the square of the width and, past a point,
/// the structure a pyramid is for; around a wider hole the axis is given up
/// as before. What is left, stated plainly: a pair of margins BOTH narrower
/// than about a ninth of the hole's short side (a fifth, when the halvings
/// land badly) is still not sampled at the coarsest level.
///
/// The wider margin of each axis and not each side, for the reason the
/// widest margin is not the narrowest: a hole against one border has no
/// margin there, and the band opposite carries that axis (the same hole
/// 700 px tall, 150 px above it and 450 below, measured 0.000 before this
/// rule). An axis already under one patch is not waited for either, so a
/// hole flush with an edge, or spanning the canvas, costs what it did.
///
/// At lift 0 (a call covering a megapixel or less) two shapes run
/// single-level at full resolution: a hole within two patches of ALL FOUR
/// borders — a selection covering the whole picture but for a thin frame, or
/// a stroke from corner to corner — where the ring has almost nothing to
/// sample from in any case, so the result is poor for a reason no pyramid
/// would fix; and, by the per-axis rule, a hole at most [`AXIS_STOP_EXTENT`]
/// across whose wider margin on one axis is 7 to 14 px. A LIFTED call does not
/// take that schedule: `inpaint_em::inpaint_plane` builds its `lift + 1`
/// levels on [`coarsenable`] alone, whatever this margin test says, because
/// a full-resolution search of several megapixels is the cost the lift
/// exists to remove.
pub(crate) fn should_coarsen<S: Sample, const C: usize>(plane: &Plane<S, C>) -> bool {
    let Some(hole) = plane.hole_box() else {
        return false;
    };
    let (x0, y0, x1, y1) = hole;
    let beside = x0.max(plane.w - 1 - x1);
    let above = y0.max(plane.h - 1 - y1);
    if !(box_coarsenable(plane, hole) && beside.max(above) > 2 * PATCH) {
        return false;
    }
    let narrower = beside.min(above);
    let short = (x1 - x0 + 1).min(y1 - y0 + 1);
    !(PATCH..=2 * PATCH).contains(&narrower) || short > AXIS_STOP_EXTENT
}

/// The widest hole (its short side, in the level's own pixels) at which
/// [`should_coarsen`] stops the pyramid to keep an axis's last source patch.
/// About nine patches: [`coarsest_iterations`] asks 86 iterations of a hole
/// this wide, and the count grows with the square, so twice the width is
/// four times the iterations on four times the targets. 64 is also what the
/// case this rule was written for needs: a megapixel hole with 150 px on both
/// sides of one axis is 61 px across where those bands are down to 10 and 11.
pub(crate) const AXIS_STOP_EXTENT: usize = 64;

/// The hole area, as its short side squared, that ONE coarsest iteration
/// settles: [`coarsest_iterations`] is `short² / 48`. A fit to the table
/// there (30 px settles in 19, 61 px in 78).
const SETTLED_PER_ITERATION: usize = 48;

/// EM iterations for the coarsest level of a pyramid whose `searched` levels
/// search (all of them at lift 0): [`em_iterations`]' 10, raised for a hole
/// too wide for 10 to settle.
///
/// 10 was chosen for the 7–14 px hole a pyramid stopped by its hole leaves.
/// A pyramid stopped by its margins leaves a wider one, and the vote is a
/// local mean: what the rim knows reaches the middle in a number of
/// iterations that grows with the SQUARE of the distance. Measured on a
/// column ramp (0.1 code a pixel at full resolution), holes of 1 to 1.2 MP,
/// the finished fill's mean absolute error in codes by the coarsest hole and
/// its count:
///
/// | coarsest hole | 10 | 20 | 40 | 160 |
/// |---|---|---|---|---|
/// | 30 x 30 | 0.778 | 0.000 | 0.000 | 0.000 |
/// | 37 x 31 | 1.680 | 0.211 | 0.000 | 0.000 |
/// | 62 x 61 | 4.767 | 2.983 | 1.087 | 0.000 |
/// | 74 x 61 | 4.770 | 3.334 | 1.835 | 0.000 |
///
/// So the count is `short² / 48` — 19 for the first row, 78 for the last
/// two — and never under 10. It is also never over `4^(searched − 1)`: the
/// coarsest level holds about that fraction of the targets of the finest
/// level that searches, so the raised count costs at most about one more
/// iteration of that level, and the coarsest level stays nearly free. A
/// single-level or two-level run therefore still takes 10. Under that
/// ceiling the last row (lift 1, four searched levels) runs 64 and measures
/// 0.621.
pub(crate) fn coarsest_iterations<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    searched: usize,
) -> u32 {
    let floor = em_iterations(0, searched);
    let Some((x0, y0, x1, y1)) = plane.hole_box() else {
        return floor;
    };
    let short = (x1 - x0 + 1).min(y1 - y0 + 1);
    let settles = (short * short).div_ceil(SETTLED_PER_ITERATION);
    let affordable = 4usize.pow(searched.saturating_sub(1).min(MAX_LEVELS) as u32);
    (settles.min(affordable) as u32).max(floor)
}

/// [`should_coarsen`]'s first two conditions without its margin test: the
/// hole's box is still wider than two patches and the window still wider than
/// four. It is what a lifted call's forced levels are built on — the plane CAN
/// be halved and still hold a hole and a patch — while the margin test only
/// says whether halving it is worth a level, which a lifted call has already
/// decided.
pub(crate) fn coarsenable<S: Sample, const C: usize>(plane: &Plane<S, C>) -> bool {
    plane
        .hole_box()
        .is_some_and(|hole| box_coarsenable(plane, hole))
}

/// [`coarsenable`] over a hole box already found, so [`should_coarsen`]
/// scans the plane once.
fn box_coarsenable<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    hole: (usize, usize, usize, usize),
) -> bool {
    let (x0, y0, x1, y1) = hole;
    let extent = (x1 - x0 + 1).max(y1 - y0 + 1);
    extent > 2 * PATCH && plane.w.min(plane.h) > 4 * PATCH
}

/// EM iterations for the level `from_coarsest` steps below the top of a
/// pyramid `levels` deep: 10 at the coarsest, 3 at the finest, 5 between. A
/// single-level run IS the coarsest and takes 10 — it has no coarser level to
/// have decided its structure. The coarsest level's 10 is a floor:
/// [`coarsest_iterations`] raises it for a wide hole.
///
/// The finest count is 3 and not the 2 this started with, and the reason is a
/// measurement. A box reduction can alias a high-frequency texture away
/// entirely — period-4 stripes reduce to a CONSTANT whenever the window's
/// origin is odd — and the finest level then has to invent the structure
/// itself. That takes three votes, not two: one to see the blurry
/// prolongation, one to commit the structure the first vote invented, and one
/// to settle the field against it. Measured on exactly that case (a 16 x 16
/// hole in period-4 stripes at an odd window origin): at 2 the fill comes back
/// visibly muddled, at 3 it is byte-exact, and 4 and 5 change nothing. The
/// finest level is the expensive one, so the count is 3 and not more.
pub(crate) fn em_iterations(from_coarsest: usize, levels: usize) -> u32 {
    if from_coarsest == 0 {
        10
    } else if from_coarsest + 1 == levels {
        3
    } else {
        5
    }
}

/// Halves a plane with a VALIDITY-WEIGHTED box mean: each coarse pixel is the
/// mean of its data-carrying children only, so the hole's filler colour never
/// contaminates a coarse pixel; a coarse pixel carries data when ANY child
/// does, and is hole only when EVERY child is hole. That asymmetry is the
/// point — it erodes the hole by one coarse pixel a level (which is what
/// makes a coarse level solvable) while keeping every scrap of real colour.
///
/// `thumbnail_reduce::box_reduced` is the master copy of the plain box mean and
/// documents the rounding; it cannot be called here — it is a single-channel
/// u8 reducer with no validity weighting and no mask to carry — so this is
/// the masked variant, named against it as the "one implementation per
/// algorithm" rule requires.
pub(crate) fn reduce<S: Sample, const C: usize>(
    fine: &Plane<S, C>,
    control: JobContext<'_>,
) -> Result<Plane<S, C>, String> {
    control.check()?;
    let (w, h) = (fine.w.div_ceil(2), fine.h.div_ceil(2));
    let mut rgb = vec![S::default(); w * h * C];
    let mut hole = vec![false; w * h];
    let mut data = vec![false; w * h];
    let mut eligibility = fine.eligibility.as_ref().map(|_| vec![true; w * h]);
    for cy in 0..h {
        control.check()?;
        for cx in 0..w {
            let i = cy * w + cx;
            let (mut children, mut with_data, mut in_hole) = (0u32, 0u32, 0u32);
            let mut sum = [0f64; C];
            let mut any = [0f64; C];
            for dy in 0..2 {
                for dx in 0..2 {
                    let (fx, fy) = (cx * 2 + dx, cy * 2 + dy);
                    if fx >= fine.w || fy >= fine.h {
                        continue;
                    }
                    let j = fy * fine.w + fx;
                    let px = &fine.rgb[j * C..j * C + C];
                    children += 1;
                    if let (Some(coarse), Some(source)) = (&mut eligibility, &fine.eligibility) {
                        coarse[i] &= source[j];
                    }
                    for (a, v) in any.iter_mut().zip(px) {
                        *a += f64::from(v.number());
                    }
                    if fine.data[j] {
                        with_data += 1;
                        for (s, v) in sum.iter_mut().zip(px) {
                            *s += f64::from(v.number());
                        }
                    }
                    if fine.hole[j] {
                        in_hole += 1;
                    }
                }
            }
            data[i] = with_data > 0;
            hole[i] = children > 0 && in_hole == children;
            // With no data-carrying child the mean of all of them is a
            // placeholder: the pixel is either hole (about to be
            // re-estimated) or has no colour at all (fully transparent), and
            // neither is ever read as a source.
            let (total, count) = if with_data > 0 {
                (sum, with_data)
            } else {
                (any, children.max(1))
            };
            for (v, t) in rgb[i * C..i * C + C].iter_mut().zip(&total) {
                *v = S::mean(*t, u64::from(count));
            }
        }
    }
    Ok(Plane {
        w,
        h,
        rgb,
        hole,
        data,
        eligibility,
    })
}

/// Writes the bilinear upsample of `coarse`'s colours into `fine`'s hole
/// pixels, leaving every pixel that carries real data exactly as it is.
///
/// Coarse cell centres map as `xc = x/2 − 1/4` (the same mapping
/// `poisson::prolongate` uses). A coarse sample with neither data nor hole
/// carries no colour and is dropped, the remaining weights renormalized, so a
/// transparent corner of the window cannot drag the fill toward black.
pub(crate) fn prolongate<S: Sample, const C: usize>(
    coarse: &Plane<S, C>,
    fine: &mut Plane<S, C>,
    control: JobContext<'_>,
) -> Result<(), String> {
    control.check()?;
    for y in 0..fine.h {
        control.check()?;
        for x in 0..fine.w {
            let i = y * fine.w + x;
            if !fine.hole[i] {
                continue;
            }
            let xc = x as f32 * 0.5 - 0.25;
            let yc = y as f32 * 0.5 - 0.25;
            let (x0, y0) = (xc.floor(), yc.floor());
            let (fx, fy) = (xc - x0, yc - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let mut acc = [0f32; C];
            let mut weight = 0f32;
            for (dx, dy, wx, wy) in [
                (0i64, 0i64, 1.0 - fx, 1.0 - fy),
                (1, 0, fx, 1.0 - fy),
                (0, 1, 1.0 - fx, fy),
                (1, 1, fx, fy),
            ] {
                let (sx, sy) = (x0 + dx, y0 + dy);
                if sx < 0 || sy < 0 || sx >= coarse.w as i64 || sy >= coarse.h as i64 {
                    continue;
                }
                let j = sy as usize * coarse.w + sx as usize;
                if !(coarse.data[j] || coarse.hole[j]) {
                    continue;
                }
                let k = wx * wy;
                for (a, v) in acc.iter_mut().zip(&coarse.rgb[j * C..j * C + C]) {
                    *a += k * v.number();
                }
                weight += k;
            }
            if weight <= 0.0 {
                continue;
            }
            for (v, a) in fine.rgb[i * C..i * C + C].iter_mut().zip(&acc) {
                *v = S::filtered(*a / weight);
            }
        }
    }
    Ok(())
}

/// Fills every hole pixel that carries no colour yet, outermost first: each
/// takes the mean of its already-known 8-neighbours, and a pixel that reaches
/// none at all takes `fallback` (the sampling ring's mean colour). See the
/// module doc — this is the coarsest level's initialization, not a fallback
/// path, and its determinism is the whole fill's.
///
/// `sdf` is the signed distance to the hole's contour, positive INSIDE
/// (`doc_select::signed_distance_field`'s convention), so ascending order
/// walks the hole from its rim to its middle.
pub(crate) fn onion_peel<S: Sample, const C: usize>(
    plane: &mut Plane<S, C>,
    sdf: &[f32],
    fallback: [S; C],
    control: JobContext<'_>,
) -> Result<(), String> {
    control.check()?;
    let mut order = Plane::<S, C>::indices(&plane.hole);
    order.sort_unstable_by(|a, b| {
        let (sa, sb) = (sdf[*a as usize], sdf[*b as usize]);
        sa.partial_cmp(&sb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });
    let mut known = plane.data.clone();
    for (index, p) in order.into_iter().enumerate() {
        control.every(index)?;
        let i = p as usize;
        let (x, y) = (i % plane.w, i / plane.w);
        let mut sum = [0f64; C];
        let mut count = 0u32;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if (dx == 0 && dy == 0)
                    || nx < 0
                    || ny < 0
                    || nx >= plane.w as i64
                    || ny >= plane.h as i64
                {
                    continue;
                }
                let j = ny as usize * plane.w + nx as usize;
                if !known[j] {
                    continue;
                }
                count += 1;
                for (s, v) in sum.iter_mut().zip(&plane.rgb[j * C..j * C + C]) {
                    *s += f64::from(v.number());
                }
            }
        }
        let out: [S; C] = if count > 0 {
            std::array::from_fn(|c| S::mean(sum[c], u64::from(count)))
        } else {
            fallback
        };
        plane.rgb[i * C..i * C + C].copy_from_slice(&out);
        known[i] = true;
    }
    Ok(())
}

/// The admissible source origins of one level.
pub(crate) struct Sources {
    /// `true` at every origin whose whole `PATCH × PATCH` window is valid.
    /// Indexed over the origin grid, `ow × oh`.
    admissible: Vec<bool>,
    /// The same set as a list, possibly strided (see [`MAX_ORIGINS`]), for
    /// the `O(1)` random draw the initialization needs.
    origins: Vec<(u32, u32)>,
    ow: usize,
    oh: usize,
}

impl Sources {
    /// How many origins the random initialization may draw from.
    pub(crate) fn len(&self) -> usize {
        self.origins.len()
    }

    /// Whether the level has no complete source patch at all — the one
    /// starvation a caller cannot fill from and must refuse.
    pub(crate) fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    /// The `i`-th admissible origin, for the random initialization's `O(1)`
    /// draw. `i` must be below [`Self::len`].
    pub(crate) fn origin(&self, i: usize) -> (u32, u32) {
        self.origins[i]
    }

    /// Whether a complete source patch may start at `(x, y)`.
    pub(crate) fn contains(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.ow
            && (y as usize) < self.oh
            && self.admissible[y as usize * self.ow + x as usize]
    }
}

/// The admissible origins over `valid`, from the integral image of that
/// plane: a window is admissible iff its `PATCH × PATCH` box sums to `P²`,
/// one add and two subtracts per origin whatever the patch size.
pub(crate) fn sources(
    valid: &[bool],
    w: usize,
    h: usize,
    control: JobContext<'_>,
) -> Result<Sources, String> {
    control.check()?;
    if w < PATCH || h < PATCH {
        return Ok(Sources {
            admissible: Vec::new(),
            origins: Vec::new(),
            ow: 0,
            oh: 0,
        });
    }
    // Row-major (w+1) x (h+1) prefix sums; u32 is ample, the window is capped
    // far below 2^32 pixels.
    let stride = w + 1;
    let mut integral = vec![0u32; stride * (h + 1)];
    for y in 0..h {
        control.check()?;
        let mut row = 0u32;
        for x in 0..w {
            row += u32::from(valid[y * w + x]);
            integral[(y + 1) * stride + x + 1] = integral[y * stride + x + 1] + row;
        }
    }
    let (ow, oh) = (w - PATCH + 1, h - PATCH + 1);
    let mut admissible = vec![false; ow * oh];
    let mut origins = Vec::new();
    for y in 0..oh {
        control.check()?;
        for x in 0..ow {
            let sum = integral[(y + PATCH) * stride + x + PATCH] + integral[y * stride + x]
                - integral[y * stride + x + PATCH]
                - integral[(y + PATCH) * stride + x];
            if sum == PATCH_AREA {
                admissible[y * ow + x] = true;
                origins.push((x as u32, y as u32));
            }
        }
    }
    if origins.len() > MAX_ORIGINS {
        // Stride the LIST, never the plane: the search may still reach any
        // admissible origin, only the random draw is thinned.
        let k = origins.len().div_ceil(MAX_ORIGINS);
        origins = origins.into_iter().step_by(k).collect();
    }
    Ok(Sources {
        admissible,
        origins,
        ow,
        oh,
    })
}

/// The origin count below which `doc_inpaint` widens the ring and asks again:
/// proportional to the hole, because 256 is a floor for "can this run at
/// all", not for "will it look like anything".
pub(crate) fn min_origins(hole_area: usize) -> usize {
    256.max(hole_area / 16)
}
