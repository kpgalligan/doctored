//! One four-connected flood walk for legacy byte and native wand and for
//! Select ▸ Grow / Similar (`selection_similar.rs`). `flood` owns the walk and
//! its job cadence; the caller's predicate decides which pixels it admits.
//! `grow` is the Magic Wand and the Paint Bucket: similarity stays anchored at
//! the single seed, and its predicate is Grow/Similar's seven-projection box
//! (`ReferenceRange::from_seed`).
//!
//! one four-connected `flood` walk (caller predicate, multi-seed) behind the byte/native wand and
//! bucket (`grow`: the seed's R, G, B, A, R−G, G−B, R−B each within ± tolerance) and Select ▸
//! Grow/Similar (`similar`: every ≥50%-covered pixel is a reference; admit when the same seven
//! projections lie in the references' range ± tolerance), both measured against Photoshop 26.10.0.
//!
//! Measurement (batch v1x-blur-wand, `scripts/photoshop-probe/blur_wand_take.py`): Photoshop 26.10.0's
//! Magic Wand and Paint Bucket, Contiguous on and off, Anti-alias off, Sample All Layers off, were
//! measured on 8-bit fixtures whose every click sits in a uniform 101×101 field (so Sample Size
//! cannot matter). Out of the declared families (seven-projection box P7, per-channel box P1,
//! luma, Euclidean and Manhattan distances, each with ≤ or < and 4- or 8-connectivity) P7 ≤ with
//! four-connectivity is the fitted member for both tools; the per-channel box this replaced
//! admitted hue shifts Photoshop refuses ((106,94,100) from grey 100 at tolerance 10). The take
//! records and the residual are in `designs/phase-9g-selections-masks-status.md`.
//!
//! Scaling: 16 bits compare u16 codes with the tolerance the host sends (8-bit t × 257), and the
//! 32-bit float bucket compares its R1 display codes the same way; R−G, G−B and R−B are taken in
//! those same codes. A gray mask or channel (`[v, v, v, max]`) reduces to |Δv| ≤ t, unchanged.
//!
//! **Ink (#1).** A CMYK document's Magic Wand and Paint Bucket compare the ink's NAIVE RGB, never
//! the view: [`grow_ink`] is `grow`'s twin over straight CMYKA16 samples and the same [`flood`],
//! each sample read through [`ink_as_rgb`] — `R = (1 − C)(1 − K)` (G from M, B from Y, A the alpha),
//! rounded to the document's depth: at 8 bits `R = 257·round((255 − c8)(255 − k8)/255)` with
//! `c8 = round(C/257)`, at 16 `R = round((65535 − C)(65535 − K)/65535)` — and then `grow`'s own
//! predicate ([`ReferenceRange`], R, G, B, A, R−G, G−B, R−B each within ± tolerance, `≤`),
//! four-connected (PN7). K enters only through the darkening it shares with every plate, so a
//! K pair or a GCR twin is admitted wherever its RGB lies inside the box (measured against Photoshop).
//! PN7 was fitted on `scripts/photoshop-probe/cmyk_select_take.py`'s take1 (0 regions mismatched
//! of 12 cases, where the shipped 11-projection ink box missed 336 and the five-plate box 136; the
//! unrounded RGB missed 16, so the 8-bit rounding is part of the rule) and confirmed by
//! `cmyk_k_take.py`'s take; the record is in `designs/cmyk-mode.md`.
//!
//! **Sample Size.** The wand's reference is the seed pixel (`sample_size` 1, Point
//! Sample, which the Paint Bucket always passes) or the per-channel truncating mean of the
//! `sample_size`-square box about it, clipped to the canvas (`ops_stats::block_mean`, the
//! eyedropper's own mean), over the codes the predicate compares: native codes, a float
//! document's R1 display codes, or the ink's naive RGB. [`WAND_SAMPLE_SIZES`] is the Sample Size
//! list (Point, 3, 5, 11, 31, 51 and 101 by 101 Average). Contiguous marks the seed full
//! whatever the averaged reference admits (`flood`'s rule); global tests every pixel.
use crate::job_control::JobContext;
use crate::selection_similar::ReferenceRange;
use crate::selection_tools::SelectionSample;
use std::collections::VecDeque;

/// The Magic Wand Sample Size list: Point Sample, then the 3, 5, 11, 31, 51
/// and 101 by 101 Average boxes, in popup order (module doc).
pub(crate) const WAND_SAMPLE_SIZES: [u32; 7] = [1, 3, 5, 11, 31, 51, 101];

/// The wand's reference codes at `seed` (module doc, **Sample Size**): the
/// seed pixel for 1, else the box mean, or the refusal of a size outside
/// [`WAND_SAMPLE_SIZES`].
pub(crate) fn reference<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    seed: (u32, u32),
    sample_size: u32,
) -> Result<[T; 4], String> {
    let (w, h) = dimensions;
    if seed.0 >= w || seed.1 >= h {
        return Err("wand seed is outside the canvas".into());
    }
    if !WAND_SAMPLE_SIZES.contains(&sample_size) {
        return Err("wand sample_size must be 1, 3, 5, 11, 31, 51 or 101".into());
    }
    let start = seed.1 as usize * w as usize + seed.0 as usize;
    if sample_size == 1 {
        return Ok(pixel(start));
    }
    crate::ops_stats::block_mean(
        dimensions,
        pixel,
        i64::from(seed.0),
        i64::from(seed.1),
        sample_size / 2,
    )
    .ok_or_else(|| "wand seed is outside the canvas".to_string())
}

/// The wand / bucket region from the single seed: global, or flooded
/// four-connected from it when `contiguous`. A pixel is admitted when its
/// seven projections each lie within ± `tolerance` of the reference's — the
/// seed's, or its `sample_size` box mean (module doc).
pub(crate) fn grow<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    seed: (u32, u32),
    tolerance: T,
    contiguous: bool,
    sample_size: u32,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let range = ReferenceRange::from_seed(reference(dimensions, &pixel, seed, sample_size)?);
    let start = seed.1 as usize * dimensions.0 as usize + seed.0 as usize;
    flood(
        dimensions,
        [start],
        |i| range.admits(pixel(i), tolerance),
        contiguous,
        job,
    )
}

/// Full coverage wherever `admit` holds. Global (`contiguous` false) tests
/// every pixel; contiguous marks every seed full, admitted or not, then walks
/// four-neighbours from them. Diagonal-only neighbours are never reached.
pub(crate) fn flood<T: SelectionSample>(
    dimensions: (u32, u32),
    seeds: impl IntoIterator<Item = usize>,
    admit: impl Fn(usize) -> bool,
    contiguous: bool,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let (w, h) = dimensions;
    let n = w as usize * h as usize;
    let mut out = vec![T::zero(); n];
    let full = T::quantize(T::MAX as f32);
    if !contiguous {
        for (i, v) in out.iter_mut().enumerate() {
            job.every(i)?;
            if admit(i) {
                *v = full;
            }
        }
        return Ok(out);
    }
    let mut queue = VecDeque::new();
    for start in seeds {
        if out[start].code() == 0. {
            out[start] = full;
            queue.push_back(start as u32);
        }
    }
    let mut head = 0;
    while let Some(index) = queue.pop_front() {
        job.every(head)?;
        if head & 65535 == 0 {
            job.report(head as f64 / n as f64, 6)?;
        }
        head += 1;
        let (x, y) = (index % w, index / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= w || ny >= h {
                continue;
            }
            let next = ny as usize * w as usize + nx as usize;
            if out[next].code() == 0. && admit(next) {
                out[next] = full;
                queue.push_back(next as u32);
            }
        }
    }
    job.check()?;
    Ok(out)
}

/// One straight CMYKA16 sample as the wand's RGBA16 codes (module doc, **Ink**): the naive
/// `(1 − ink)(1 − K)` per plate, rounded through 8-bit codes on an 8-bit document.
pub(crate) fn ink_as_rgb(p: [u16; 5], depth_bits: u32) -> [u16; 4] {
    let [c, m, y, k, a] = p;
    let plate = |v: u16| -> u16 {
        if depth_bits <= 8 {
            let v8 = (u32::from(v) + 128) / 257;
            let k8 = (u32::from(k) + 128) / 257;
            let r8 = ((255 - v8) * (255 - k8) + 127) / 255;
            (257 * r8) as u16
        } else {
            let product = (65535 - u64::from(v)) * (65535 - u64::from(k));
            ((product + 32767) / 65535) as u16
        }
    };
    [plate(c), plate(m), plate(y), a]
}

/// The CMYK wand / bucket region from the single seed (module doc, **Ink**):
/// `grow`'s rule over [`ink_as_rgb`] of straight CMYKA16 samples at the
/// document's `depth_bits`, through the same [`flood`]; a `sample_size` box
/// averages those naive RGB codes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grow_ink(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [u16; 5],
    seed: (u32, u32),
    tolerance: u16,
    contiguous: bool,
    sample_size: u32,
    depth_bits: u32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let rgb = |i: usize| ink_as_rgb(pixel(i), depth_bits);
    let range = ReferenceRange::from_seed(reference(dimensions, rgb, seed, sample_size)?);
    let start = seed.1 as usize * dimensions.0 as usize + seed.0 as usize;
    flood(
        dimensions,
        [start],
        |i| range.admits(ink_as_rgb(pixel(i), depth_bits), tolerance),
        contiguous,
        job,
    )
}
