//! Content-aware inpainting on the layered document: spot healing (the
//! footprint is the hole) and Content-Aware Fill (the selection is the hole).
//!
//! Both are the same pipeline, and it is short to state: split the hole into
//! connected components; around each, take a window of its bounding box plus
//! the sampling ring; PatchMatch-inpaint the hole from the valid pixels in
//! that ring (`patchmatch`, driven level by level from `inpaint_em`); then
//! hand the result to the ONE healing write-back,
//! [`crate::doc_heal::heal_window_into_pixels_with_context`], which Poisson-blends the
//! invented texture to the illumination it lands in. The two
//! algorithms compose exactly: PatchMatch matches TEXTURE and is indifferent
//! to level, the membrane solve fixes LEVEL and cannot touch texture, and
//! because the solve is exact for a zero mismatch a fill that was already
//! correct is left byte-identical.
//!
//! # The Spot Healing Brush's Type
//!
//! Spot healing takes a Type, `spot_heal_type::SpotHealType`: Content-Aware
//! (the default, everything this doc describes), Proximity Match (the whole
//! region copied rigidly from the best-matching neighbouring offset, its
//! search ring widened by `inpaint_plan::requested_ring`) and Create Texture
//! (blocks of the stroke's own detrended pixels tiled over it). Each replaces
//! the PatchMatch step in `fill_component` and nothing else: the caps, the
//! plan, the admission and the healing write-back are the same for all
//! three, and Content-Aware Fill is always Content-Aware.
//!
//! # What each op reads
//!
//! - The HOLE — the set PatchMatch re-estimates — is read from the overlay's
//!   ALPHA for spot healing (its RGB is deliberately ignored: that overlay
//!   carries pure coverage, there is no sampled source) and from the
//!   selection mask for the fill, and the two take it at different
//!   thresholds. A brush dab is cut hard at `>= 128`; a selection is taken
//!   wherever it touches at all (`> 0`), so a feathered edge is filled and
//!   faded rather than stepped. [`Caller::hole_threshold`] holds the reason.
//! - The SOURCE the inpaint samples is the plate `retouch_sampling`'s mode
//!   names — layer `idx`'s own canvas-placed pixels for Current, the
//!   projection cut at the target for Current and Below, the whole projection
//!   for All — built once per call by `inpaint_source::sampling_source`, the
//!   one plate builder every retouch gesture shares. A composite mode
//!   therefore costs one composite, once per call, tiled where the cut stack
//!   allows it.
//! - The DESTINATION whose illumination the fill takes is always layer
//!   `idx`'s own pixels, and its alpha is never touched: a hole over
//!   transparent pixels stays transparent (there is nothing to blend to, and
//!   the write-back's contract is that alpha does not move). The one
//!   exception is opt-in: a Content-Aware Fill request with
//!   `fill_transparent` (phase 2b-1, Auto-Blend's Content-Aware Fill
//!   Transparent Areas) sets the hole's alpha to full on the destination
//!   before the fill writes it (`precision_inpaint::opaque_hole`), so the
//!   fill shows there; RGB, the source plate and the coverage weighting are
//!   untouched by it.
//! - The WRITE WEIGHT differs between the two, and follows the same split.
//!   Content-Aware Fill weights the write-back by the selection's own
//!   coverage bytes, so a feathered edge genuinely blends; spot healing takes
//!   `doc_heal`'s full weight inside the α = 128 contour and nothing outside
//!   it, exactly as the healing brush does — hardness shrinks the footprint,
//!   it does not fade the heal. Because the hole runs to the same threshold
//!   the weight starts from, the fill's contribution rises CONTINUOUSLY from
//!   0: measured on a 200 px plasma with a 40 px ramp, the mean distance the
//!   fill moves a pixel is 7 code values at coverage 1-24, 50 at 64-111, 70
//!   at 112-127, 77 at 128-143 and 138 at 215-255. Cutting the hole at 128
//!   and weighting by the byte inside it made that sequence 0, 0, 0, 77, 138
//!   — a hard edge on the 50 % contour of an edge the user asked to be soft,
//!   which is what `a_feathered_selection_blends_across_its_whole_ramp`
//!   guards. [`Caller`] is the two-way distinction and nothing else; the
//!   entry point is also what words every refusal, since a stroke and a
//!   selection cannot be told to do the same thing about a cap.
//!
//! # Per component, and what that costs
//!
//! Each component is filled from its OWN neighbourhood, so a scatter of
//! blemishes costs the sum of their own boxes and not one box around all of
//! them, and two blemishes far apart cannot borrow each other's colour. The
//! source image is read once, before the first component: components are
//! filled from the ORIGINAL surroundings and never from each other's results.
//! Two blemishes closer together than a patch may therefore see each other's
//! unfilled pixels inside a target patch's distance — never as a SOURCE,
//! since every hole is excluded from the valid set.
//!
//! What is emphatically NOT per component is the layer buffer, nor the caps.
//! `inpaint` clones the layer's pixels once and threads that copy through
//! every component, `doc_heal::heal_layer`'s rule and for its reason: a fresh
//! document per component cost a full layer copy each time — ~20 ms per speck
//! on a 100-megapixel canvas, twenty seconds of `memcpy` for a thousand-speck
//! dust selection before any inpainting. The caps are below.
//!
//! # The sampling ring
//!
//! `ring` is the width in pixels of the band around the hole the fill may
//! copy from; 0 means automatic. It is then subject to three rules, in order,
//! and every one of them is visible to the user in prose because the ring is
//! a user-facing number:
//!
//! 1. **Widened from below by the hole itself**, never under `3·PATCH` and
//!    never under half the hole's own radius. An 8-px ring around a
//!    1000 × 1000 hole would offer a few thousand origins for a million
//!    targets, which does not look like texture — it looks like tiling. It
//!    applies to an EXPLICIT ring too (around a megapixel hole a requested
//!    21, 48, 100 and 282 are all the same 282), and it is a QUALITY rule
//!    rather than a cost one — see the cost section for what that does and
//!    does not mean.
//! 2. **Narrowed, uniformly and in advance, to fit the caps.** If the hole
//!    dilated by the ring would take the call past
//!    [`MAX_INPAINT_TARGET_PIXELS`] (scaled by the call's lift, below) — or
//!    its windows past
//!    [`MAX_INPAINT_PLAN_PIXELS`] — EVERY component's ring is narrowed by
//!    the same number of steps of a fifth until the total fits, down to
//!    `3·PATCH`; only if even that busts a cap is the call refused. A
//!    mildly tiled fill is better than a refusal. `inpaint_plan` does this
//!    before any component is filled, and its module doc holds the three
//!    defects that placement fixes: a budget spent first-come-first-served
//!    refused a scatter of specks that fits comfortably, refused it only
//!    after paying for most of the fill, and sized every window from the
//!    ring the caller ASKED for rather than the one the narrowing chose.
//! 3. **Widened again, per level, if the origins are starved.** Below
//!    `patchmatch::min_origins` the ring doubles and the admissible set is
//!    rebuilt, up to the whole window. That widening never grows the WINDOW,
//!    which was sized from rule 1 — it only relaxes the restriction inside
//!    it, so it can never bust a cap that was already checked.
//!
//! # The caps, and the order they are checked in
//!
//! ```text
//! INPAINT_SEARCH_PIXELS      the CALL's separate PARTS, and what one level of one part SEARCHES
//! MAX_INPAINT_TARGET_PIXELS  the CALL's hole DILATED by the ring — the real work, x 4^lift
//! MAX_INPAINT_PLAN_PIXELS    the CALL's working WINDOWS — the boxes, summed
//! MAX_SOLVE_WINDOW_PIXELS    one component's window — a MEMORY bound (doc_heal)
//! ```
//!
//! The covered pixel count itself is not capped (it once was, at a
//! megapixel): [`inpaint_lift`] turns it into the number of pyramid levels
//! the call skips before it searches, and "Holes over a megapixel" below is
//! what that does. The refusals a large hole can still meet are the four
//! above, the search guard that section describes, and memory admission.
//!
//! Each caps a quantity that actually drives cost. None of them caps ONE
//! hole's bounding box, which was the obvious thing to cap and the wrong
//! one: a three-pixel scratch, a wire, a stray hair — content-aware fill's
//! single most common use — has a box of nearly the whole photograph and an
//! area of a few thousand pixels, and refusing that would be worse than the
//! transient. What the boxes are capped by is their SUM, and only because
//! the dilated cap cannot see them: twenty such scratches on one canvas are
//! 3.2 M dilated (inside the work cap) and 190 megapixels of window, which
//! measured 16.3 s.
//!
//! **The first three bound the CALL, not one part of it**, and the difference
//! is the whole point of having them: a magic-wand, Colour Range or Select
//! Subject selection is routinely dozens of components, so measured per
//! component the caps bounded nothing — four disjoint 620 × 620 squares, half
//! again a megapixel, were accepted at full resolution and cost the sum. The
//! parts are bounded once, before the component walk; the covered count that
//! sets the lift and the dilated count are summed by `inpaint_plan`, over
//! every component, before any of them is filled, so those four squares now
//! run at lift 1 between them.
//! `MAX_SOLVE_WINDOW_PIXELS` stays per component on purpose — it bounds the
//! working buffers ONE solve allocates, and those are freed before the next.
//!
//! **Every one of them is checked before a pixel moves**, and that is a
//! contract, not an implementation detail: a caller may try a selection and
//! be told no without paying for a fill. What a refusal costs is the
//! component walk plus `inpaint_plan`'s first, arithmetic ladder — 7 ms for
//! a 90 000-part dust selection, 15 ms for 143 canvas-spanning scratches —
//! and only a selection that clears every cheap bound goes on to pay for
//! distance transforms (0.22 s for the one-component comb of
//! `a_refused_fill_costs_far_less_than_a_fill`, where the fill it refused
//! costs 1.6 s). It used to cost the whole fill, 13 s of frozen main thread
//! in the app to be told no, because the budget was spent inside the loop
//! that did the work; and then it cost a full-resolution distance transform
//! per component, which for a dust selection is the same disaster in a
//! different place — 28.7 s to refuse a million one-pixel specks.
//!
//! # Holes over a megapixel: the lift
//!
//! A call covering more than [`INPAINT_SEARCH_PIXELS`] used to be refused.
//! It now runs LIFTED. [`inpaint_lift`] is the smallest `L` with
//! `covered ≤ 1,000,000·4^L` over the CALL's covered pixels (1 to 4 MP, 2 to
//! 16 MP, 3 to 64 MP), and in every part of the call
//! (`inpaint_em::inpaint_plane`):
//!
//! - the pyramid is built at least `L + 1` levels deep, on
//!   `patchmatch::coarsenable` alone — the margin test that leaves a hole
//!   touching all four edges of its window at one level is skipped for those
//!   levels, because that single level would be the full-resolution search
//!   the lift exists to avoid;
//! - with `first = min(L, depth − 1)`, level `first` and the coarser ones run
//!   the ordinary schedule, and each level finer than `first` takes the
//!   coarser field doubled, measures it once and votes once, with no search;
//! - the work cap becomes `MAX_INPAINT_TARGET_PIXELS·4^L`, the factor the
//!   search shrank by;
//! - a part that cannot be reduced `L` times (its window is under four
//!   patches wide at the level it stopped at) and still holds more than
//!   `INPAINT_SEARCH_PIXELS` hole pixels there is refused before any search
//!   ([`search_cap_message`]). So no level of any part searches more than a
//!   megapixel of hole, whatever the call covers.
//!
//! `L = 0` takes none of those steps and refuses in the same sentences. Its
//! bytes are not what they were before `inpaint_lift`, though, at any lift:
//! the same batch changed how deep a pyramid is built and how long its
//! coarsest level runs (`patchmatch::should_coarsen`'s per-axis rule and
//! `patchmatch::coarsest_iterations`), because a hole with one narrow pair of
//! margins was being filled from its wide sides only.
//!
//! **What a lifted fill is, and is not.** The search and its EM iterations
//! run at `1/2^L` scale, so structure larger than about `PATCH·2^L` pixels
//! is matched. Finer texture is real source texture — the last vote copies
//! full-resolution source pixels through the doubled field — but it is not
//! matched in phase to the hole's edge, and nothing searches for it at full
//! resolution. No photograph was qualified above a megapixel. The tests are
//! synthetic (`content_aware_large`): a flat field comes back byte for byte;
//! a column ramp with a 1200 x 1000 hole comes back to a mean absolute error
//! of 0.12 codes (largest 4); a corner-to-corner band of 1.14 MP in the
//! same ramp, 4.60 (largest 31); a 1200 x 1100 hole in a random two-colour
//! 3 px texture keeps the picture's mean to 1.0 code and 0.91 of its mean
//! squared horizontal step; and the same seed gives the same bytes. The
//! typed kernel on the same ramp ×257 with narrower margins (1600 x 1300, a
//! 1200 x 1000 hole) measures 1.01 byte codes against the 4 that batch
//! declared, and a 1 MP hole at lift 0 in the same picture 0.003
//! (`precision_inpaint`); the byte fill of that lift-0 hole is exact. Before
//! the per-axis rule and the sized coarsest count those three were 5.15,
//! 22.5 and 22.4.
//!
//! **Cost.** Measured once on this machine with no other build running
//! (`content_aware_cost_tests`' `mod bench` twins, `--release`, bounds
//! declared before the run): a 2000 x 2000 hole — 4 MP, the most lift 1
//! takes — in a 3000 x 2600 column ramp fills in 11.6 s against a 30 s
//! bound, and a 3.63 MP corner-to-corner band in the same picture in 16.2 s
//! against 60 s. The band costs more per pixel because its forced pyramid is
//! two levels deep, so its searched level is the coarsest and runs the
//! coarsest schedule. Those times are with the per-axis rule and the sized
//! coarsest count; before them the same bench measured 14.3 s and 16.7 s
//! (the hole's pyramid is now one level shallower, with 78 iterations at its
//! coarsest level; the band's schedule did not change).
//!
//! **Memory.** The admission formulas did not change, and one lifted case per
//! depth was measured against them (`examples/inpaint_lift_admission.rs`):
//! the byte record is in `doc_content_aware`'s `# Admission`, the typed one
//! in `precision_inpaint`'s.
//!
//! # Measured cost
//!
//! **What costs time is the region DILATED by its ring, plus the WINDOWS the
//! parts are worked in** — the two quantities [`MAX_INPAINT_TARGET_PIXELS`]
//! and [`MAX_INPAINT_PLAN_PIXELS`] bound, and the reason those are the caps
//! they are. It is emphatically not the selected count: the two are within a
//! small factor for a compact blob and nowhere near each other for a long
//! thin one, which is content-aware fill's most common shape. A linear fit
//! over the rows below gives **0.1 s per megapixel of window plus 1.2 s per
//! megapixel of dilated region**, and it is the model the caps are sized
//! from. `content_aware_tests`, `content_aware_cost_tests` and a probe run
//! beside them, this machine, `--release`:
//!
//! | case | selected | dilated | windows | total |
//! |---|---|---|---|---|
//! | 2000 x 1500 photograph, a 300 x 300 hole | 90 k | ~0.4 M | 0.4 M | 0.53 s |
//! | 2000 x 1500, a 3 px diagonal scratch | 6.5 k | ~0.23 M | 3 M | 0.60 s |
//! | 5000 x 5000, ONE 3 px scratch corner to corner | 15 k | ~1.0 M | 25 M | 4.2 s |
//! | 5000 x 5000, FOUR such scratches | 44 k | ~2.9 M | 57 M | 9.1 s |
//! | **a megapixel, the most lift 0 takes**: a 1000 x 1000 hole, ring 48 | 1 M | ~3 M | 2.4 M | 5.4 s |
//! | the same at ring 512 | 1 M | ~3 M | 4.1 M | 5.2 s |
//! | 2000 x 1500, 1131 12-px specks, ring 21 | 163 k | ~1.7 M | 17 M | 3.6 s |
//! | the same at ring 512 | 163 k | ~1.7 M | 17 M | 3.8 s |
//! | 3000 x 3000, ten 3 px scratches | 78 k | ~1.6 M | 68 M | 9.8 s |
//! | **the worst permitted call found**: ten 17 px scratches | 395 k | ~3.9 M | 71 M | 12.1 s |
//! | REFUSED: 90 000 one-pixel specks | 90 k | — | — | 0.007 s |
//! | REFUSED: 143 canvas-spanning scratches | 288 k | — | — | 0.015 s |
//! | REFUSED on the work cap, one component | 163 k | — | — | 0.22 s |
//!
//! Read across the first three rows: a scratch selecting a sixth of what the
//! compact hole selects costs eight times as much, because its window is the
//! whole photograph and a 70 px ring around a 7 000 px diagonal is a
//! megapixel of work. So **the worst case for a permitted call through a
//! megapixel is about twelve seconds**, not the five a megapixel hole costs
//! — inside the earlier fifteen-second synchronous-work budget (a lifted
//! call's cost is in "Holes over a megapixel"). Final app Apply now runs
//! this pipeline through an immutable background job; explicit checkpoints
//! cancel its planning, PatchMatch and membrane solve without a partial edit.
//!
//! It follows that the advice to a caller with a slow fill is not simply
//! "select less". What to make smaller is the region PLUS its ring, or the
//! number of separate parts: a shorter scratch is cheaper, a thinner one is
//! not, and splitting a long scratch into separate calls buys nothing at all
//! (the parts dilate to the same band and their boxes still overlap).
//!
//! **And it costs the same at the edge of the frame as in the middle of it**,
//! which for a while it did not: `patchmatch::should_coarsen` lost the whole
//! pyramid for any selection touching a border, at 2.3x the time and 2.9x the
//! error. That doc has the measurements;
//! `a_hole_at_the_canvas_edge_costs_like_one_in_the_middle` asserts the ratio
//! so it cannot rot again.
//!
//! **The ring the caller PASSES is not a cost knob**, which is a different
//! claim from the one above and also true — but only because rule 2 makes it
//! so, and for a while it did not. At the megapixel cap every ring costs the
//! same to within noise (5.4 s at a requested 48, widened to 282 by rule 1,
//! and 5.2 s at 512), because rule 1 raises a small ring from below and one
//! big window is one big window either way. On a SCATTERED selection that
//! reasoning does not apply and the claim was simply false: every component's
//! window was sized from the ring the caller asked for, so 1131 specks at
//! `ring: 512` built 1131 windows of a megapixel each and took 78.9 s against
//! 3.0 s at 21 — a 26x cost multiplier on a parameter the schema calls free,
//! paid on the main thread. `inpaint_plan` now sizes each window from the
//! ring the narrowing SETTLES on, so the same selection is 3.6 s at 21 and
//! 3.8 s at 512. `the_ring_does_not_drive_the_cost_at_the_cap` and
//! `the_ring_does_not_drive_the_cost_of_a_scattered_selection` assert both
//! ratios, so neither claim can rot.
//!
//! An earlier draft had the cost model backwards twice over. It said "32 s at
//! the automatic ring against 6.5 s at 48" — measured through an app macOS
//! had already demoted with App Nap, which makes every later op in that
//! process ~5x slower (the app's `AppActivity.swift` holds the measurement
//! and the assertion that prevents it) — and it then generalized a
//! 2000 x 1500 scratch measurement into "it costs like its area", which the
//! 5000 x 5000 rows above falsify by a factor of ten.
//!
//! one pipeline per connected component — window, sampling ring, PatchMatch, then
//! `doc_heal::heal_window_into_pixels_with_context`. Owns the caps (`INPAINT_SEARCH_PIXELS`,
//! `MAX_INPAINT_TARGET_PIXELS`, `MAX_INPAINT_PLAN_PIXELS`), which bound the whole CALL and not one
//! component of it, `inpaint_lift` (a hole over a megapixel searches at a reduced scale
//! instead of being refused), and their refusal sentences — one per cap per `Caller`, because a spot-heal
//! stroke has neither the selection nor the ring a fill's wording names; the two
//! starved-neighbourhood refusals are told apart the same way (nothing to sample from at all,
//! versus a band of real picture narrower than one source patch, which is the common case and used
//! to be reported as the other); `Caller` also decides the hole's coverage threshold, since a
//! selection's feathered ramp must be filled and faded while a brush dab is cut hard at 128. The
//! reduced `preview` pipeline went with its always-false flag. The union
//! read polls per visited tile; the reserves charge `COMPONENT_BYTES` per
//! `doc_heal::component_count_bound` start before the walk and per component found after it
//! (`heal_layer_unlocked`'s rule). `inpaint_scratch_bound(doc, components, rect_pixels,
//! window)` charges the destination clone, the windows' union rect on the layer, at `4·rect` (0
//! before the plan) instead of the whole layer; `precision_inpaint`'s `# Admission` holds the S1/S3
//! footprints that keep the 10 B canvas term, and `Caller::hole_threshold` is `pub(crate)` for the
//! fill's re-admission. Both take the call's `lift`: a lifted call's window term is
//! `LIFTED_WINDOW_BYTES`, and the post-plan reserve passes the plan's own lift.

use crate::job_control::JobContext;
use image::RgbaImage;

use crate::doc::DrDocument;
use crate::doc_heal::{Component, MAX_SOLVE_WINDOW_PIXELS};
use crate::doc_lock::EditKind;
use crate::inpaint_em::hole_distance;
use crate::inpaint_plan::{plan, ComponentPlan};
use crate::layer_raster::ALLOCATION;
use crate::layer_rect::{canvas_rect_on_layer, rect_image_with_job, rect_offset};
use crate::patchmatch::{Plane, PATCH};
use crate::poisson::COVERED_THRESHOLD;
use crate::retouch_sampling::RetouchSampling;
use crate::spot_heal_type::{SpotHealParams, SpotHealType};

/// The most hole pixels ONE pyramid level of one part will SEARCH — about
/// 1000 x 1000. It is the count of PatchMatch targets, hence very nearly the
/// whole cost of the search; the measured cost at this number is in the
/// module doc.
///
/// It was the largest hole one call would fill (`MAX_INPAINT_HOLE_PIXELS`).
/// A call covering more is no longer refused: [`inpaint_lift`] moves its
/// search up the pyramid until the searched level is back under this number
/// (the module doc's "Holes over a megapixel"). Three things are still
/// measured against it: that lift, the count of separate parts one call may
/// have ([`parts_cap_message`]), and the admission's component-record clamp
/// ([`inpaint_scratch_terms`]), which the parts refusal keeps honest.
pub const INPAINT_SEARCH_PIXELS: u64 = 1_000_000;

/// How many pyramid levels a call covering `covered` hole pixels skips before
/// it searches: the smallest `L` in `0..patchmatch::MAX_LEVELS` with
/// `covered <= INPAINT_SEARCH_PIXELS << (2 * L)`, and `MAX_LEVELS - 1` when
/// there is none. 0 through a megapixel (the pipeline as it always ran), 1 to
/// 4 MP, 2 to 16 MP, 3 to 64 MP.
///
/// `covered` is the CALL's total, every separate part together, for the
/// reason the caps are totals: what it scales is the work the call does. The
/// work cap scales with it ([`MAX_INPAINT_TARGET_PIXELS`]` << (2 * L)`), and
/// every part of the call runs the same lift.
pub fn inpaint_lift(covered: u64) -> u32 {
    let top = crate::patchmatch::MAX_LEVELS as u32 - 1;
    (0..=top)
        .find(|lift| covered <= INPAINT_SEARCH_PIXELS << (2 * lift))
        .unwrap_or(top)
}

/// The largest hole-plus-ring ONE CALL will work over at [`inpaint_lift`] 0;
/// a lifted call's budget is this `<< (2 * lift)`, the same factor its search
/// is reduced by. The ring is what the origin list, the integral image and
/// the distance transform are sized from, so this is the bound on everything
/// the hole count does not already bound.
///
/// It is a total over every component rather than a test each of them takes
/// separately, for the same reason: what it bounds is the work the call does.
/// `inpaint_plan` measures it over the whole selection before any of it is
/// filled, and narrows every component's ring by the same steps to fit — so a
/// refusal costs a distance transform and not a fill, and which speck the
/// component walk reached first cannot decide the answer.
pub const MAX_INPAINT_TARGET_PIXELS: u64 = 4_000_000;

/// The largest total of WORKING WINDOWS one call will build. It bounds the
/// quantity [`MAX_INPAINT_TARGET_PIXELS`] deliberately does not: a component's
/// window is its bounding BOX plus the ring, and a long thin scratch's box is
/// nearly the whole photograph while it dilates to a fraction of it. Twenty
/// three-pixel scratches across a 3000 x 3000 picture dilate to 3.8 M — inside
/// the work cap — and come to 180 megapixels of window planes, distance
/// transform and integral image.
///
/// The number comes from the time budget, not from taste. Measured on this
/// machine a call costs about `0.1 s` per megapixel of window plus `1.2 s`
/// per megapixel of dilated region, so 80 megapixels of window with the work
/// cap's 4 megapixels of region is ~13 s — inside the 15 s the phase allows
/// a blocking Apply, and the only quantity that was previously unbounded.
/// Without it, thirty three-pixel scratches across a 3000 x 3000 photograph
/// (3.2 M dilated, comfortably inside the work cap) measured 16.3 s.
///
/// Like the work cap it is a total over the call, and `inpaint_plan` narrows
/// every ring by the same steps to fit it — though narrowing rarely helps
/// here, since a window is mostly its component's bounding BOX. What makes
/// it fit is fewer or shorter parts: three corner-to-corner scratches on a
/// 5000 x 5000 scan are 75 megapixels and fill, four are 100 and are
/// refused, and each of the four on its own is 25 and fills.
pub const MAX_INPAINT_PLAN_PIXELS: u64 = 80_000_000;

/// Which entry point a call came in through. The caps are one set of numbers
/// with one home; the sentences that report them are not, because the two
/// gestures have nothing in common — a menu command over a selection, with a
/// ring field in its sheet, and a brush stroke that has neither. A spot-heal
/// stroke told to "select less, or use a narrower ring" is being sent to a
/// command it did not run and a control it does not have; `doc_heal`'s window
/// limit already words the healing brush's bound for the healing brush's
/// gesture, and this is how the inpainting caps do the same.
///
/// It decides one thing about the pixels as well, and only one:
/// [`Caller::soft_write`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caller {
    /// Content-Aware Fill: the region is a selection, the ring is a field in
    /// a sheet, and the work is asked for from a menu.
    Fill,
    /// The Spot Healing Brush: the region is a stroke's footprint, and the
    /// tool exposes neither a selection nor a ring.
    SpotHeal,
}

impl Caller {
    /// Whether the caller's coverage BYTES weight the write-back, or the hard
    /// cut at the α = 128 contour does. True for Content-Aware Fill alone:
    /// there the pixels outside Ω are the original image, so a feathered
    /// selection edge genuinely blends. Spot healing's coverage is a brush
    /// dab, and `doc_heal`'s hard-cut rule applies to it exactly as it does
    /// to the healing brush — Hardness shrinks the footprint, it does not
    /// fade the heal.
    fn soft_write(self) -> bool {
        self == Caller::Fill
    }

    /// The coverage byte at or above which a pixel is part of the HOLE — the
    /// set PatchMatch re-estimates and the membrane solve then corrects.
    ///
    /// It is the same distinction as [`Caller::soft_write`] and it has to
    /// be, or the write-back steps where the coverage ramps. Content-Aware
    /// Fill takes every pixel the selection touches at all (1), because its
    /// coverage is a SELECTION whose feathered edge is a request for a fade:
    /// filling only `≥ 128` and weighting by the byte put the whole ramp
    /// below the contour at weight 0 and the first pixel above it at 0.502,
    /// so a soft-edged selection stepped by half the fill's own effect
    /// across one pixel — measured at 9 code values on a 128 px plasma.
    /// Inpainting the ramp too costs the ramp's own pixels (they count
    /// towards [`inpaint_lift`]'s covered total, which is honest — they are
    /// being re-estimated) and makes the write weight continuous from 0.
    ///
    /// Spot healing's coverage is a brush DAB, and `doc_heal`'s hard cut at
    /// the α = 128 contour applies to it exactly as it does to the healing
    /// brush: hardness shrinks the footprint, it does not fade the heal.
    pub(crate) fn hole_threshold(self) -> u8 {
        match self {
            Caller::Fill => 1,
            Caller::SpotHeal => COVERED_THRESHOLD,
        }
    }
}

/// How well a refusal knows the number it is quoting. The cheap ladder in
/// `inpaint_plan` refuses a scattered selection from bounding boxes alone —
/// deliberately, since measuring it exactly is the cost that pass exists to
/// avoid — so the sentence says "more than" rather than inventing precision
/// it does not have.
pub(crate) enum Measured {
    /// Counted, pixel by pixel, on a distance transform.
    Exactly(u64),
    /// A rigorous floor from the components' bounding boxes.
    AtLeast(u64),
}

impl Measured {
    /// The phrase that goes where a refusal names the size of the call.
    fn phrase(&self) -> String {
        match self {
            Measured::Exactly(total) => format!("covers {total} pixels"),
            Measured::AtLeast(total) => format!("covers more than {total} pixels"),
        }
    }
}

/// The knobs both entry points share.
struct Fill<'a> {
    control: JobContext<'a>,
    strength: f32,
    ring: u32,
    seed: u64,
    kind: SpotHealType,
    params: SpotHealParams,
    sample: RetouchSampling,
    ignore_adjustments: bool,
    caller: Caller,
    eligibility: Option<&'a [bool]>,
    source_plate: Option<&'a RgbaImage>,
    /// Content-Aware Fill Transparent Areas' pad (phase 2b-1, the
    /// module doc's opt-in exception): the hole is made opaque on the
    /// destination before the fill writes it. False for spot healing.
    fill_transparent: bool,
}

impl DrDocument {
    /// Spot healing: PatchMatch-inpaints the footprint `overlay`'s ALPHA
    /// marks from a ring of valid pixels around it, then Poisson-blends the
    /// result into layer `idx` exactly as [`DrDocument::heal_layer`] does.
    ///
    /// `overlay` is the canvas-frame premultiplied RGBA8 buffer
    /// `dr_doc_painting_layer` takes (`w`/`h` must equal the canvas size) but
    /// only its alpha is read — there is no sampled source, so its RGB
    /// carries no meaning. The alpha is COVERAGE and nothing else: the
    /// write-back is `doc_heal`'s hard cut at the α = 128 contour, so a soft
    /// tip heals a smaller footprint rather than a fainter one, exactly as
    /// the healing brush does. `strength` is clamped to [0, 1] and scales the
    /// write-back; `ring` is the sampling ring in px (0 = automatic) subject
    /// to the module doc's three rules; `seed` makes the result reproducible;
    /// `kind` is the Type (`spot_heal_type`: Content-Aware, Create Texture or
    /// Proximity Match), which replaces only the synthesis step;
    /// `sample`/`ignore_adjustments` choose the sampling plate
    /// (`retouch_sampling`).
    ///
    /// `Err` (a message for the user) on a cap or a starved sampling region.
    /// `Ok(None)` for an out-of-range `idx`, `w`/`h` not the canvas size, a
    /// short overlay, a non-finite `strength`, an empty footprint, a layer
    /// extent that misses the canvas, or no pixel actually changing.
    // The parameter list deliberately mirrors `dr_doc_spot_heal_layer_job`'s C
    // signature one-for-one; bundling them into a struct would only move the
    // count somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spot_heal_layer_job(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        strength: f32,
        ring: u32,
        seed: u64,
        kind: SpotHealType,
        sample: RetouchSampling,
        ignore_adjustments: bool,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.spot_heal_with(
            idx,
            overlay,
            (w, h),
            strength,
            (ring, seed),
            (kind, crate::spot_heal_type::SHIPPED),
            (sample, ignore_adjustments),
            control,
        )
    }

    /// The Photoshop take's seam (`examples/spot_heal_variants.rs`): spot
    /// healing with an explicit `params` candidate, at the canvas size,
    /// strength 1, the automatic ring, seed 0, sampling Current, no job
    /// control. Same results and refusals as [`Self::spot_heal_layer_job`].
    #[doc(hidden)]
    pub fn spot_heal_variant(
        &self,
        idx: usize,
        overlay: &[u8],
        kind: SpotHealType,
        params: SpotHealParams,
    ) -> Result<Option<Self>, String> {
        self.spot_heal_with(
            idx,
            overlay,
            (self.width, self.height),
            1.0,
            (0, 0),
            (kind, params),
            (RetouchSampling::Current, false),
            JobContext::NONE,
        )
    }

    /// [`Self::spot_heal_layer_job`]'s body with the type's parameters
    /// explicit.
    #[allow(clippy::too_many_arguments)]
    fn spot_heal_with(
        &self,
        idx: usize,
        overlay: &[u8],
        (w, h): (u32, u32),
        strength: f32,
        (ring, seed): (u32, u64),
        (kind, params): (SpotHealType, SpotHealParams),
        (sample, ignore_adjustments): (RetouchSampling, bool),
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            let Some(n) = canvas_len(doc, w, h) else {
                return Ok(None);
            };
            if overlay.len() < n * 4 {
                return Ok(None);
            }
            control.check()?;
            let coverage: Vec<u8> = (0..n)
                .map(|i| {
                    control.every(i)?;
                    Ok(overlay[i * 4 + 3])
                })
                .collect::<Result<_, String>>()?;
            inpaint(
                doc,
                idx,
                &coverage,
                Fill {
                    strength,
                    ring,
                    seed,
                    kind,
                    params,
                    sample,
                    ignore_adjustments,
                    caller: Caller::SpotHeal,
                    eligibility: None,
                    source_plate: None,
                    fill_transparent: false,
                    control,
                },
            )
        })
    }

    /// Content-Aware Fill: PatchMatch-inpaints the region a canvas-sized u8
    /// coverage mask marks (the selection convention — 0 outside, 255 inside,
    /// intermediate = anti-aliased edge), sampled from a ring around it, then
    /// Poisson-blends the fill to the surrounding illumination.
    ///
    /// Every covered pixel is part of the hole ([`Caller::hole_threshold`]),
    /// and the mask's own bytes weight the write-back, so a feathered edge is
    /// filled and faded across its whole ramp and nothing outside the mask
    /// moves.
    ///
    /// `ring`, `seed`, `sample` and `ignore_adjustments` are
    /// `spot_heal_layer_job`'s. Same refusals, plus `Ok(None)` for an empty
    /// mask.
    ///
    /// Shared byte synthesis with an optional captured source and independent
    /// source permissions. Both arms pass the request's compiled `source_mask`
    /// as `eligibility` when it has one; only the new-layer arm passes a
    /// `source_plate` (its captured sampling source). `fill_transparent` is
    /// the in-place request's pad (the module doc's opt-in exception); the
    /// new-layer arm's temporary plate passes false.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn content_aware_fill_source_job(
        &self,
        idx: usize,
        mask: &[u8],
        w: u32,
        h: u32,
        ring: u32,
        seed: u64,
        sample: RetouchSampling,
        ignore_adjustments: bool,
        eligibility: Option<&[bool]>,
        source_plate: Option<&RgbaImage>,
        fill_transparent: bool,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            let Some(n) = canvas_len(doc, w, h) else {
                return Ok(None);
            };
            if eligibility.is_some_and(|e| e.len() != n)
                || source_plate.is_some_and(|p| p.dimensions() != (w, h))
            {
                return Err("content-aware source extent mismatch".into());
            }
            if mask.len() < n {
                return Ok(None);
            }
            inpaint(
                doc,
                idx,
                &mask[..n],
                Fill {
                    strength: 1.0,
                    ring,
                    seed,
                    kind: SpotHealType::ContentAware,
                    params: crate::spot_heal_type::SHIPPED,
                    sample,
                    ignore_adjustments,
                    caller: Caller::Fill,
                    eligibility,
                    source_plate,
                    fill_transparent,
                    control,
                },
            )
        })
    }
}

/// The canvas pixel count, or `None` when `w`/`h` are not the canvas size or
/// the canvas is empty.
fn canvas_len(doc: &DrDocument, w: u32, h: u32) -> Option<usize> {
    if w != doc.width || h != doc.height {
        return None;
    }
    (w as usize)
        .checked_mul(h as usize)
        .filter(|n| *n > 0 && n.checked_mul(4).is_some())
}

/// What one component's fill needs to read, gathered once for the whole call.
struct Job<'a> {
    /// The caller's coverage bytes, canvas-sized.
    coverage: &'a [u8],
    /// `coverage >= 128` anywhere — every component's hole, so no component
    /// can sample from another's unknown pixels.
    hole_all: &'a [bool],
    /// The image the inpaint samples: the composite, or the layer's own
    /// pixels placed in canvas space.
    source: &'a RgbaImage,
    fill: &'a Fill<'a>,
}

/// The driver: validate, choose the source image, check the caps that bound
/// the whole CALL, then split the coverage into components and fill each into
/// one shared copy of the layer's pixels.
fn inpaint(
    doc: &DrDocument,
    idx: usize,
    coverage: &[u8],
    fill: Fill<'_>,
) -> Result<Option<DrDocument>, String> {
    let control = fill.control;
    control.report(0.0, 1)?;
    let Some(layer) = doc.raster_layer(idx) else {
        return Ok(None);
    };
    if !fill.strength.is_finite() {
        return Ok(None);
    }
    // The canvas-frame coverage must be able to reach a layer pixel at all —
    // the same intersection `DrDocument::painting_layer` (`doc.rs`, the
    // master copy) makes before touching anything.
    let (lw, lh) = layer.pixels.dimensions();
    let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    let reach_x = (-off_x).max(0) < (i64::from(doc.width) - off_x).min(i64::from(lw));
    let reach_y = (-off_y).max(0) < (i64::from(doc.height) - off_y).min(i64::from(lh));
    if !reach_x || !reach_y {
        return Ok(None);
    }
    // What counts as hole is the caller's, and only the caller's: a brush dab
    // is cut hard at the α = 128 contour, a selection is taken wherever it
    // touches at all so its feathered ramp is filled and faded rather than
    // stepped. See `Caller::hole_threshold`.
    let threshold = fill.caller.hole_threshold();
    let (cw, ch) = (doc.width as usize, doc.height as usize);
    let hole_all: Vec<bool> = coverage
        .iter()
        .enumerate()
        .map(|(i, c)| {
            control.every(i)?;
            Ok(*c >= threshold)
        })
        .collect::<Result<_, String>>()?;
    let covered = hole_all.iter().filter(|c| **c).count() as u64;
    if covered == 0 {
        return Ok(None);
    }
    // No ceiling on the covered count itself: a call over a megapixel is
    // lifted (`inpaint_lift`, summed over the components by `inpaint_plan`).
    // What is refused here, before the walk, is a call in more separate parts
    // than the admission's record clamp prices.
    // The records before the walk: its proven bound, not the hole count.
    let starts = crate::doc_heal::component_count_bound(&hole_all, cw, ch, control)?;
    if starts > INPAINT_SEARCH_PIXELS {
        return Err(parts_cap_message(fill.caller, starts));
    }
    control.reserve(inpaint_scratch_bound(doc, starts, 0, 0, 0))?;
    control.report(0.03, 1)?;
    let source = if let Some(plate) = fill.source_plate {
        std::borrow::Cow::Borrowed(plate)
    } else {
        let Some(source) = crate::inpaint_source::sampling_source(
            doc,
            idx,
            fill.sample,
            fill.ignore_adjustments,
            inpaint_scratch_bound(doc, starts, 0, 0, 0),
            control,
        )?
        else {
            return Ok(None);
        };
        std::borrow::Cow::Owned(source)
    };
    // A SAMPLE-ABLE pixel is one that carries colour, and alpha's only job
    // here is to answer that — the RGB beside it is straight, so a pixel at
    // alpha 204 is as good a source as one at 255. Testing `== 255` made this
    // whole op unusable on any composite that is nowhere fully opaque: a
    // single layer at 80 % opacity flattens to alpha 204 everywhere, every
    // pixel was disqualified, and the call was refused with a sentence
    // claiming the surroundings were transparent when nothing in the picture
    // was. The threshold is the crate's one 50 % rule, the same contour the
    // hole itself is cut at.
    control.check()?;
    if !source.pixels().any(|p| p.0[3] >= COVERED_THRESHOLD) {
        return Err(if !fill.sample.is_layer_only() {
            // Distinct from NOTHING_TO_SAMPLE, which is about the region's
            // own neighbourhood: this one is about the whole sampled image.
            "there is nothing to sample from: every pixel in the picture is transparent."
                .to_string()
        } else {
            "the layer has no pixels solid enough to sample from — they are transparent, or \
             nearly so; sample Current and Below or All Layers, or fill on the layer that \
             holds the photograph."
                .to_string()
        });
    }
    // Every component's window and ring, and the work cap, measured BEFORE a
    // single pixel is filled: a refusal must not cost what a fill costs. See
    // `inpaint_plan`'s module doc for the two defects that pass exists for.
    control.report(0.06, 2)?;
    let components = crate::doc_heal::components_with_context(&hole_all, cw, ch, control)?;
    let plans = plan(
        &components,
        (cw, ch),
        fill.ring,
        fill.caller,
        fill.kind,
        control.range(0.07, 0.18),
        inpaint_scratch_bound(doc, components.len() as u64, 0, 0, 0),
    )?;
    // ONE clone of the layer for the whole call, threaded through every
    // component — `doc_heal::heal_layer`'s rule, and the reason it has one.
    // Chaining a fresh document per component instead cost a full layer copy
    // each time, which on a 100-megapixel canvas is 400 MB per speck of a
    // dust-removal selection. Since 13B-2c-2a the clone is only the union of
    // the windows on the layer: a window pixel is reachable through it
    // exactly when it is on-canvas and on-layer. A union that misses the
    // layer cannot move a byte, so it answers `Ok(None)` before any solve
    // (the status doc's 13B-2c-2a decision). The post-plan reserve charges
    // that clone by its own rect, not the whole layer.
    let union = plans
        .iter()
        .fold(None::<(i64, i64, i64, i64)>, |union, sized| {
            let (x0, y0, ww, wh) = sized.window;
            let window = (x0 as i64, y0 as i64, (x0 + ww) as i64, (y0 + wh) as i64);
            Some(match union {
                None => window,
                Some((ux0, uy0, ux1, uy1)) => (
                    ux0.min(window.0),
                    uy0.min(window.1),
                    ux1.max(window.2),
                    uy1.max(window.3),
                ),
            })
        });
    let rect = union.and_then(|u| canvas_rect_on_layer((doc.width, doc.height), layer, u));
    control.reserve(inpaint_scratch_bound(
        doc,
        components.len() as u64,
        rect.map_or(0, |(_, _, rw, rh)| u64::from(rw) * u64::from(rh)),
        plans
            .iter()
            .map(ComponentPlan::window_pixels)
            .max()
            .unwrap_or(0),
        // Every part of the call runs the one lift its plan carries.
        plans.first().map_or(0, |sized| sized.lift),
    ))?;
    let Some(rect) = rect else {
        return Ok(None);
    };
    let job = Job {
        coverage,
        hole_all: &hole_all,
        source: &source,
        fill: &fill,
    };
    control.report(0.18, 3)?;
    let mut pixels =
        rect_image_with_job(&layer.pixels, rect, control)?.ok_or_else(|| ALLOCATION.to_string())?;
    let offset = rect_offset(layer, rect);
    control.check()?;
    // The opt-in pad before any write (`precision_inpaint::opaque_hole`, the
    // one copy): the write-back leaves a transparent destination alone.
    let plane = pixels.dimensions();
    let mut changed = fill.fill_transparent
        && crate::precision_inpaint::opaque_hole::<u8, 3>(
            &mut pixels,
            plane,
            offset,
            (doc.width, doc.height),
            |ci| coverage[ci] >= threshold,
            255,
        );
    let total_work = plans
        .iter()
        .map(ComponentPlan::window_pixels)
        .sum::<u64>()
        .max(1);
    let mut completed_work = 0u64;
    for (component, sized) in components.iter().zip(&plans) {
        let component_control = control.range(
            0.18 + 0.81 * completed_work as f64 / total_work as f64,
            0.18 + 0.81 * (completed_work + sized.window_pixels()) as f64 / total_work as f64,
        );
        changed |= fill_component(
            (doc.width, doc.height),
            &mut pixels,
            offset,
            component,
            sized,
            &job,
            component_control,
        )?;
        completed_work += sized.window_pixels();
    }
    control.report(0.99, 4)?;
    if !changed {
        // Identity result: refuse rather than mint an unchanged copy.
        return Ok(None);
    }
    let (x, y, rw, rh) = rect;
    let raw = pixels.into_raw();
    let Some(raster) =
        layer
            .pixels
            .with_rect_with_job(x, y, rw, rh, raw.as_chunks::<4>().0, control)?
    else {
        return Ok(None);
    };
    Ok(doc.with_layer_raster(idx, raster))
}

/// Fills one component into `pixels`: the window planes, the inpaint and the
/// healing write-back. `sized` is the window and ring `inpaint_plan` already
/// measured and fitted for this component. Returns whether any byte moved.
fn fill_component(
    canvas: (u32, u32),
    pixels: &mut RgbaImage,
    offset: (i32, i32),
    component: &Component,
    sized: &ComponentPlan,
    job: &Job,
    control: JobContext<'_>,
) -> Result<bool, String> {
    control.report(0.0, 3)?;
    let cw = canvas.0 as usize;
    let (x0, y0, ww, wh) = sized.window;
    let ring = sized.ring;

    // --- the window's planes, at full resolution -------------------------
    let n = ww * wh;
    let mut rgb = vec![0u8; n * 3];
    let mut solid = vec![false; n];
    let mut hole_any = vec![false; n];
    let mut eligibility = job.fill.eligibility.map(|_| vec![false; n]);
    for wy in 0..wh {
        control.check()?;
        for wx in 0..ww {
            let i = wy * ww + wx;
            let p = (y0 + wy) * cw + x0 + wx;
            let px = job.source.get_pixel((x0 + wx) as u32, (y0 + wy) as u32).0;
            rgb[i * 3..i * 3 + 3].copy_from_slice(&px[..3]);
            // Solid enough to carry colour, not necessarily fully opaque:
            // see the guard in `inpaint` for what testing `== 255` cost.
            solid[i] = px[3] >= COVERED_THRESHOLD;
            hole_any[i] = job.hole_all[p];
            if let (Some(out), Some(mask)) = (&mut eligibility, job.fill.eligibility) {
                out[i] = mask[p];
            }
        }
    }
    let mut hole = vec![false; n];
    let mut write = vec![0u8; n];
    for &p in &component.pixels {
        let (cx, cy) = (p as usize % cw, p as usize / cw);
        let i = (cy - y0) * ww + cx - x0;
        hole[i] = true;
        // Only this component's own coverage: a neighbouring blob inside the
        // same window is another component's business. Whether that coverage
        // WEIGHTS the write-back or is merely the hard cut at α = 128 is
        // `Caller::soft_write`'s question — see its own comment.
        write[i] = if job.fill.caller.soft_write() {
            job.coverage[p as usize]
        } else {
            255
        };
    }

    let data: Vec<bool> = (0..n).map(|i| solid[i] && !hole_any[i]).collect();
    let plane = Plane::<u8, 3> {
        w: ww,
        h: wh,
        rgb,
        hole,
        data,
        eligibility,
    };
    // ONE distance transform over the window, which the finest pyramid
    // level takes as its own. `inpaint_plan` measured this component's
    // dilated area on its own copy and dropped it; the transform is the
    // cheapest stage in the pipeline and holding one f32 plane per
    // component would not be.
    let sdf = hole_distance(&plane.hole, ww, wh, control)?;
    // The Type replaces the synthesis and nothing else (`spot_heal_type`);
    // Create Texture alone reads which hole pixels carry real colour.
    let (source, region) = crate::spot_heal_type::synthesize(
        job.fill.kind,
        job.fill.params,
        plane,
        (job.fill.kind == SpotHealType::CreateTexture).then_some(&solid[..]),
        ring,
        job.fill.seed,
        Some(sdf),
        job.fill.caller,
        sized.lift,
        control.range(0.05, 0.85),
    )?;
    for (weight, covered) in write.iter_mut().zip(&region) {
        if *covered < COVERED_THRESHOLD {
            *weight = 0;
        }
    }
    crate::doc_heal::heal_window_into_pixels_with_context(
        canvas,
        pixels,
        offset,
        (x0 as i64, y0 as i64, ww, wh),
        &source,
        &region,
        &write,
        job.fill.strength,
        control.range(0.85, 1.0),
    )
}

// ---- the cap refusals, one sentence per cap per caller --------------------
//
// The limits, the numbers and the arithmetic have one home each (above, and
// `doc_heal::MAX_SOLVE_WINDOW_PIXELS`); what branches here is only the
// sentence, because the two callers are asked for in different gestures and a
// refusal has to name something the person holding that gesture can change.
// See [`Caller`].

/// More separate parts than [`INPAINT_SEARCH_PIXELS`]: `starts` is
/// `doc_heal::component_count_bound`'s figure, an upper bound on the parts
/// counted before the walk, which is why the sentence says "up to". The
/// admission prices at most that many component records, so a call past it is
/// refused before a record is made.
pub(crate) fn parts_cap_message(caller: Caller, starts: u64) -> String {
    match caller {
        Caller::Fill => format!(
            "the selection is in up to {starts} separate parts; content-aware fill works on at \
             most {} separate parts in one call. Select fewer parts, or fill them in pieces.",
            INPAINT_SEARCH_PIXELS
        ),
        Caller::SpotHeal => format!(
            "the stroke is in up to {starts} separate parts; spot healing works on at most {} \
             separate parts in one call. Heal it in shorter strokes.",
            INPAINT_SEARCH_PIXELS
        ),
    }
}

/// One part of a lifted call that cannot be reduced as far as its lift asks —
/// its working box is under four patches wide at the scale it stopped at — and
/// still holds `pixels` hole pixels there, more than [`INPAINT_SEARCH_PIXELS`].
/// `inpaint_em::inpaint_plane` returns it before any search, so no level of
/// any part ever searches more than that many.
pub(crate) fn search_cap_message(caller: Caller, pixels: u64) -> String {
    match caller {
        Caller::Fill => format!(
            "one part of the selection has {pixels} pixels to search at the smallest scale its \
             working box allows, because that box is too narrow to work at a smaller one; \
             content-aware fill searches at most {} pixels at one scale. Select less of that \
             part, or fill it in pieces.",
            INPAINT_SEARCH_PIXELS
        ),
        Caller::SpotHeal => format!(
            "one part of the stroke has {pixels} pixels to search at the smallest scale its \
             working box allows, because that box is too narrow to work at a smaller one; spot \
             healing searches at most {} pixels at one scale. Heal it in shorter strokes.",
            INPAINT_SEARCH_PIXELS
        ),
    }
}

/// The work cap — `budget`, [`MAX_INPAINT_TARGET_PIXELS`] scaled by the
/// call's [`inpaint_lift`] — busted by the call's dilated total, with every
/// ring already narrowed to `floor` — the narrowest the rules will use.
/// The advice is the one that works: the ring has been narrowed as far as it
/// goes, so what is left to change is the region.
pub(crate) fn target_cap_message(
    caller: Caller,
    total: Measured,
    floor: f32,
    budget: u64,
) -> String {
    let amount = total.phrase();
    match caller {
        Caller::Fill => format!(
            "the region plus the sampling ring around it {amount}, with the ring \
             already narrowed as far as it goes ({floor:.0} px); content-aware fill works on at \
             most {budget} in one call, counting every separate part of the selection together. \
             Select less, or fill it in pieces."
        ),
        Caller::SpotHeal => format!(
            "the stroke plus the band of pixels it heals from {amount}, with that \
             band already as narrow as it goes ({floor:.0} px); spot healing works on at most \
             {budget} in one call, counting every part of the stroke together. Heal it in \
             shorter strokes, or with a smaller brush."
        ),
    }
}

/// The refusal when the neighbourhood holds no pixel to copy from AT ALL —
/// every pixel around the region is transparent, part of the region, or off
/// the canvas.
///
/// Kept distinct from [`narrow_band_message`], which is the far more common
/// case and used to be reported with this sentence: a selection with three
/// solid, opaque, on-canvas pixels of picture around it was told its
/// surroundings were transparent when they plainly were not, and given
/// nothing to act on. The two are told apart by the `valid` plane
/// `inpaint_em::build_sources` already returns: this one is for an empty one.
pub(crate) fn nothing_to_sample_message(caller: Caller) -> String {
    match caller {
        Caller::Fill => "there is nothing to sample from: every pixel around the selection is \
                         transparent, part of the selection, or outside the canvas."
            .to_string(),
        Caller::SpotHeal => "there is nothing to sample from: every pixel around the stroke is \
                             transparent, part of it, or outside the canvas."
            .to_string(),
    }
}

/// The refusal when there ARE pixels to copy from but the band they form is
/// narrower than one source patch, so not one complete patch fits in it.
///
/// `patchmatch::sources` admits an origin only when its whole `PATCH x PATCH`
/// window is valid, so a band under seven pixels yields an empty origin set
/// however opaque and on-canvas it is. Measured on a 200 x 200 fully opaque
/// document with a centred rectangular selection: bands of 3, 5 and 6 px
/// refuse here and a 7 px band fills. It is reachable whenever a selection
/// comes within six pixels of the picture's edge on every side — "remove the
/// subject" where the subject nearly fills the frame, or Select All followed
/// by a small Contract — so the sentence names the width and says what to do.
pub(crate) fn narrow_band_message(caller: Caller) -> String {
    match caller {
        Caller::Fill => format!(
            "the band of picture around the selection is narrower than one source patch \
             ({PATCH} px), so there is no complete patch to copy from — leave at least {PATCH} \
             px of unselected picture around the region, or select less."
        ),
        Caller::SpotHeal => format!(
            "the band of picture around the stroke is narrower than one source patch \
             ({PATCH} px), so there is no complete patch to copy from — use a smaller brush, \
             or heal a spot with more picture around it."
        ),
    }
}

/// [`MAX_INPAINT_PLAN_PIXELS`] busted by the total of the call's working
/// windows, with every ring already at
/// [`RING_MIN`](crate::inpaint_plan::RING_MIN). Distinct from the work cap
/// because the thing to change is distinct: the windows are bounding BOXES,
/// so what makes them smaller is fewer or shorter parts, and thinning the
/// selection or narrowing the ring does nothing at all.
pub(crate) fn plan_cap_message(caller: Caller, total: u64) -> String {
    let megapixels = |v: u64| v as f64 / 1e6;
    match caller {
        Caller::Fill => format!(
            "the separate parts of the selection span {:.0} megapixels of working area between \
             them, with the sampling ring already as narrow as it goes; content-aware fill \
             works over at most {:.0} megapixels in one call. Each part needs buffers over \
             its whole bounding box, so fill fewer parts at a time, or shorter ones — \
             thinning the selection will not help.",
            megapixels(total),
            megapixels(MAX_INPAINT_PLAN_PIXELS)
        ),
        Caller::SpotHeal => format!(
            "the separate parts of the stroke span {:.0} megapixels of working area between \
             them, with the band they heal from already as narrow as it goes; spot healing \
             works over at most {:.0} megapixels in one call. Heal it in shorter strokes.",
            megapixels(total),
            megapixels(MAX_INPAINT_PLAN_PIXELS)
        ),
    }
}

/// `doc_heal::MAX_SOLVE_WINDOW_PIXELS` busted by one component's window. The
/// number and the memory it stands for are `doc_heal`'s; only the gesture
/// being refused is this file's business. Both arms are also
/// `precision_inpaint::estimate_window`'s window refusal, which
/// answers before any component is planned: the Fill arm for the byte fill
/// admission and the typed Content-Aware Fill, the SpotHeal arm for the
/// typed Spot Heal.
pub(crate) fn window_cap_message(caller: Caller, ww: usize, wh: usize, pixels: u64) -> String {
    let megapixels = |v: u64| v as f64 / 1e6;
    match caller {
        Caller::Fill => format!(
            "the region plus its sampling ring spans a {ww} x {wh} box ({:.1} megapixels); \
             filling needs working buffers over that box and is limited to {:.0} megapixels — \
             fill it in pieces, or select a smaller region.",
            megapixels(pixels),
            megapixels(MAX_SOLVE_WINDOW_PIXELS)
        ),
        Caller::SpotHeal => format!(
            "the stroke plus the band it heals from spans a {ww} x {wh} box ({:.1} megapixels); \
             healing needs working buffers over that box and is limited to {:.0} megapixels — \
             heal it in shorter strokes.",
            megapixels(pixels),
            megapixels(MAX_SOLVE_WINDOW_PIXELS)
        ),
    }
}

/// Working allocation reservation excluding retained inputs/caches, conservatively
/// counting each window's RGB/masks, geometric pyramid, NNF and Poisson levels
/// at 128 bytes per pixel. Sequential components share one destination clone.
/// Canvas planes and bounded component lists add 10 bytes per canvas pixel.
/// `components` counts the `precision_inpaint::COMPONENT_BYTES` records: the
/// `doc_heal::component_count_bound` before the walk, the walk's own count
/// after it (`heal_layer_unlocked`'s rule). Either is at most
/// [`INPAINT_SEARCH_PIXELS`] once `inpaint` has passed its parts refusal, so
/// `inpaint_scratch_terms`' `min(·, INPAINT_SEARCH_PIXELS)` charges every
/// record there is, and still holds for the fill admission's `area`. `rect_pixels` is the destination clone: the
/// windows' union rect on the layer, which `inpaint` reads and rewrites as
/// RGBA8 (it charged the whole layer's bytes before). It is 0 before the
/// plan, when no clone exists yet.
pub(crate) fn inpaint_scratch_bound(
    doc: &DrDocument,
    components: u64,
    rect_pixels: u64,
    window: u64,
    lift: u32,
) -> u64 {
    let n = u64::from(doc.width) * u64::from(doc.height);
    inpaint_scratch_terms(n, rect_pixels.saturating_mul(4), components, window, lift)
}

/// Bytes per WINDOW pixel a call lifted by at least one level
/// ([`inpaint_lift`] ≥ 1) is charged, where a lift-0 call is charged 136. A
/// lifted call holds the full-resolution window only as its RGBA, masks and
/// the Poisson write-back; its pyramid, NNF and search planes start a level
/// up, at a quarter of the area or less, which is what 136 priced at full
/// size. Measured (`doc_content_aware`'s `# Admission`): 136 charged
/// the whole-canvas hole of a 24 MP Content-Aware crop at 3.4× its peak; at
/// 40 that crop plans 1.35× it and the 4 MP lift-1 example 1.47×.
pub(crate) const LIFTED_WINDOW_BYTES: u64 = 40;

/// [`inpaint_scratch_bound`]'s arithmetic over its four measured terms, for
/// an admission that knows them before any document exists
/// (`doc_content_aware`'s): `n·10 + layer + min(components,
/// INPAINT_SEARCH_PIXELS)·COMPONENT_BYTES + min(window, cap)·w + 8 MiB`,
/// `w` 136 at `lift` 0 and [`LIFTED_WINDOW_BYTES`] above it.
pub(crate) fn inpaint_scratch_terms(
    canvas_pixels: u64,
    layer_bytes: u64,
    components: u64,
    window: u64,
    lift: u32,
) -> u64 {
    let window_bytes = if lift == 0 { 136 } else { LIFTED_WINDOW_BYTES };
    canvas_pixels
        .saturating_mul(10)
        .saturating_add(layer_bytes)
        .saturating_add(
            components
                .min(INPAINT_SEARCH_PIXELS)
                .saturating_mul(crate::precision_inpaint::COMPONENT_BYTES),
        )
        .saturating_add(
            window
                .min(MAX_SOLVE_WINDOW_PIXELS)
                .saturating_mul(window_bytes),
        )
        .saturating_add(8 * 1024 * 1024)
}
