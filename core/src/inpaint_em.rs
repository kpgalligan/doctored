//! The pyramid and EM loop one component's fill runs through: build the
//! levels, solve coarse to fine, and hand back the inpainted window and the
//! region the membrane solve is to run over.
//!
//! `doc_inpaint` owns the document, the window and the caps; `patchmatch` owns
//! the arithmetic and every parameter; this is the loop that puts them
//! together, split out of `doc_inpaint` because a driver and a policy are two
//! things. Nothing here knows about `DrDocument`.
//!
//! `inpaint_em.rs` is the pyramid/EM driver.

use crate::doc_inpaint::{
    narrow_band_message, nothing_to_sample_message, search_cap_message, Caller,
    INPAINT_SEARCH_PIXELS,
};
use crate::doc_select::signed_distance_field_with_context;
use crate::inpaint_plan::RING_MIN;
use crate::inpaint_sample::Sample;
use crate::job_control::JobContext;
use crate::patchmatch::{self, Plane};
use crate::patchmatch_nnf::Nnf;
use crate::poisson::COVERED_THRESHOLD;

/// The EM driver: a pyramid over `plane`, solved coarse to fine, returning
/// the inpainted colour (`C` channels a pixel: RGB, or a CMYK document's four
/// ink codes) and the solve region (the hole grown by one pixel).
///
/// The returned RGB carries the hole AND that one-pixel outer ring, both
/// written by the vote. The ring is what makes the colour adaptation
/// possible: the membrane's boundary condition is `destination − source` on
/// the region's contour, so the source has to carry the inpaint's own
/// extension there, or the mismatch would be zero and the solve would have
/// nothing to correct.
///
/// `lift` is the call's `doc_inpaint::inpaint_lift`; 0 is the schedule above
/// and nothing else. With `lift > 0` (the call covers over a megapixel):
///
/// - **Forced depth.** The pyramid is built `lift + 1` levels deep on
///   `patchmatch::coarsenable` alone, past `should_coarsen`'s margin test, so
///   a hole whose box touches all four edges of its window still gets the
///   levels its lift needs. Past those, `should_coarsen` decides as before.
/// - **Lifted levels.** With `first = min(lift, depth − 1)`, the levels finer
///   than `first` do no search: each prolongates the coarser estimate, takes
///   the coarser field doubled (`Nnf::new`, which measures every target once)
///   and votes once. Only when the coarser level left no field — it was
///   starved and skipped — does a lifted level run one `search` from a random
///   field before that vote, because a vote needs a field. Level `first` and
///   everything coarser run their ordinary schedule.
/// - **The search guard.** A plane that cannot be reduced `lift` times
///   (`depth − 1 < lift`: its window is under four patches wide at the level
///   it stopped at) and still holds more than `INPAINT_SEARCH_PIXELS` hole
///   pixels at that level is refused with `search_cap_message` before any
///   search, so no level searches more than that many.
pub(crate) fn inpaint_plane<S: Sample, const C: usize>(
    plane: Plane<S, C>,
    ring: f32,
    seed: u64,
    mut fine_sdf: Option<Vec<f32>>,
    caller: Caller,
    lift: u32,
    control: JobContext<'_>,
) -> Result<(Vec<S>, Vec<u8>), String> {
    control.check()?;
    let lift = lift as usize;
    let mut levels = vec![plane];
    while levels.len() < patchmatch::MAX_LEVELS {
        control.check()?;
        let last = &levels[levels.len() - 1];
        if !(patchmatch::should_coarsen(last)
            || (levels.len() <= lift && patchmatch::coarsenable(last)))
        {
            break;
        }
        let coarse = patchmatch::reduce(last, control)?;
        levels.push(coarse);
    }
    let depth = levels.len();
    // The finest level that searches. Everything finer is lifted.
    let first = lift.min(depth - 1);
    if depth - 1 < lift {
        let searched = levels[first].hole.iter().filter(|h| **h).count() as u64;
        if searched > INPAINT_SEARCH_PIXELS {
            return Err(search_cap_message(caller, searched));
        }
    }
    // The share of this call's progress the searched levels report over. All
    // of it at lift 0, so those reports are what they always were. A lifted
    // call keeps a fifth for its lifted levels: each costs one distance and
    // one vote per target against the searched level's dozen sweeps over a
    // quarter as many, which is the same order, not a multiple of it.
    let searched_share = if first == 0 { 1.0 } else { 0.8 };
    let mut coarser: Option<Nnf<S, C>> = None;
    let mut voted: Vec<[S; C]> = Vec::new();
    for level in (0..depth).rev() {
        control.check()?;
        let (fine, rest) = levels.split_at_mut(level + 1);
        let plane = &mut fine[level];
        // The caller has already measured the finest level's contour; every
        // coarser one is its own. The loop runs coarsest FIRST, so the take
        // has to be guarded rather than filtered — taking unconditionally and
        // filtering afterwards dropped the caller's vector on the first
        // iteration and recomputed the full-resolution transform at level 0.
        let sdf = match if level == 0 { fine_sdf.take() } else { None } {
            Some(sdf) => sdf,
            None => hole_distance(&plane.hole, plane.w, plane.h, control)?,
        };
        // The ring is measured in THIS level's pixels and never narrower than
        // three patches: a band thinner than a patch has no admissible origin
        // at all, so the coarse levels of a deep pyramid would starve.
        let level_ring = (ring / (1 << level) as f32).max(RING_MIN);
        let (sources, valid) = build_sources(plane, &sdf, level_ring, control)?;
        if level + 1 == depth {
            let fallback = mean_colour(plane, &valid);
            patchmatch::onion_peel(plane, &sdf, fallback, control)?;
        } else {
            patchmatch::prolongate(&rest[0], plane, control)?;
        }
        if sources.is_empty() && level == 0 {
            // Two different nothings, and the caller can act on the
            // difference: no pixel to copy from at all, or pixels whose band
            // is thinner than one patch. `valid` is the plane the ring
            // widening ended on — by here it has already grown to the whole
            // window — so a non-empty one means the second.
            return Err(if plane.eligibility.is_some() {
                format!("source_mask leaves no complete {} px source patch around the selection; include a wider source region or reduce exclusions", patchmatch::PATCH)
            } else if valid.iter().any(|v| *v) {
                narrow_band_message(caller)
            } else {
                nothing_to_sample_message(caller)
            });
        }
        let seeded = coarser.is_some();
        let Some(mut nnf) = Nnf::new(
            plane,
            sources,
            seed,
            level as u32,
            coarser.as_ref(),
            control,
        )?
        else {
            // Nothing to do at this level: either the reduction erased the
            // hole, or the hole leaves no room for a complete source patch at
            // this scale. Either way its estimate — the masked mean, or the
            // onion peel — is what the next finer level starts from, and a
            // COARSE level is an accelerator, never a requirement.
            coarser = None;
            continue;
        };
        let hole = Plane::<S, C>::indices(&plane.hole);
        voted.resize(hole.len(), [S::default(); C]);
        if level < first {
            // A lifted level: the coarser field, doubled and measured by
            // `Nnf::new`, voted once. No search unless there is no coarser
            // field to stand on.
            let done =
                searched_share + (1.0 - searched_share) * (first - 1 - level) as f64 / first as f64;
            control.report(done, 3)?;
            if !seeded {
                nnf.search(plane, control)?;
            }
            nnf.vote(plane, &hole, &mut voted[..hole.len()], control)?;
            for (k, &p) in hole.iter().enumerate() {
                let i = p as usize * C;
                plane.rgb[i..i + C].copy_from_slice(&voted[k]);
            }
            coarser = Some(nnf);
            continue;
        }
        // The coarsest level's count follows the hole it was left with: a
        // pyramid stopped by its margins leaves one wider than 10 iterations
        // settle (`patchmatch::coarsest_iterations`). `depth − first` levels
        // search, which is what bounds that count.
        let iterations = if level + 1 == depth {
            patchmatch::coarsest_iterations(plane, depth - first)
        } else {
            patchmatch::em_iterations(depth - 1 - level, depth)
        };
        for iteration in 0..iterations {
            // Geometric pixel work: finer levels receive proportionally more progress.
            let done = searched_share
                * (4f64.powi(-((level - first) as i32)) / 4.0)
                * (1.0 + 3.0 * iteration as f64 / iterations as f64);
            control.report(done, 3)?;
            nnf.search(plane, control)?;
            nnf.vote(plane, &hole, &mut voted[..hole.len()], control)?;
            for (k, &p) in hole.iter().enumerate() {
                let i = p as usize * C;
                plane.rgb[i..i + C].copy_from_slice(&voted[k]);
            }
        }
        coarser = Some(nnf);
    }

    let plane = &levels[0];
    let mut region: Vec<u8> = plane
        .hole
        .iter()
        .map(|c| if *c { 255 } else { 0 })
        .collect();
    crate::doc_select::offset_contour_with_context(&mut region, plane.w, plane.h, 1.0, control)?;
    let mut source = plane.rgb.clone();
    if let Some(nnf) = coarser.as_ref() {
        let write: Vec<u32> = region
            .iter()
            .enumerate()
            .filter_map(|(i, r)| (*r >= COVERED_THRESHOLD).then_some(i as u32))
            .collect();
        voted.resize(write.len(), [S::default(); C]);
        nnf.vote(plane, &write, &mut voted[..write.len()], control)?;
        for (k, &p) in write.iter().enumerate() {
            let i = p as usize * C;
            source[i..i + C].copy_from_slice(&voted[k]);
        }
    }
    Ok((source, region))
}

/// The signed distance to the hole's contour over a window: positive inside,
/// negative outside, from `doc_select::signed_distance_field` — the crate's
/// ONE exact Euclidean transform, and the reason neither the ring nor the
/// onion peel needs a second one.
pub(crate) fn hole_distance(
    hole: &[bool],
    w: usize,
    h: usize,
    control: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    control.check()?;
    let mask: Vec<u8> = hole.iter().map(|c| if *c { 255 } else { 0 }).collect();
    signed_distance_field_with_context(&mask, w, h, control)
}

/// The admissible sources of one level, widening the ring while the origins
/// are starved (rule 3 of `doc_inpaint`'s ring rules), with the valid plane it came
/// from. The set can come back EMPTY — a coarse level whose hole leaves no
/// room for a complete patch — which only the finest level may refuse on.
fn build_sources<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    sdf: &[f32],
    ring: f32,
    control: JobContext<'_>,
) -> Result<(patchmatch::Sources, Vec<bool>), String> {
    let floor = patchmatch::min_origins(plane.hole.iter().filter(|h| **h).count());
    let span = (plane.w + plane.h) as f32;
    let mut ring = ring;
    loop {
        control.check()?;
        let valid: Vec<bool> = plane
            .data
            .iter()
            .zip(sdf)
            .enumerate()
            .map(|(i, (d, s))| *d && *s >= -ring && plane.eligibility.as_ref().is_none_or(|e| e[i]))
            .collect();
        let sources = patchmatch::sources(&valid, plane.w, plane.h, control)?;
        if sources.len() >= floor || ring >= span {
            return Ok((sources, valid));
        }
        ring *= 2.0;
    }
}

/// The mean colour of the pixels a mask marks — the onion peel's last resort
/// for a hole that reaches no known pixel at all.
fn mean_colour<S: Sample, const C: usize>(plane: &Plane<S, C>, mask: &[bool]) -> [S; C] {
    let mut sum = [0f64; C];
    let mut count = 0u64;
    for (i, inside) in mask.iter().enumerate() {
        if !*inside {
            continue;
        }
        count += 1;
        for (s, v) in sum.iter_mut().zip(&plane.rgb[i * C..i * C + C]) {
            *s += f64::from(v.number());
        }
    }
    if count == 0 {
        return [S::default(); C];
    }
    std::array::from_fn(|c| S::mean(sum[c], count))
}
