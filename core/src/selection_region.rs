//! One four-connected flood walk for legacy byte and native wand and for
//! Select ▸ Grow / Similar (`selection_similar.rs`). `flood` owns the walk and
//! its job cadence; the caller's predicate decides which pixels it admits.
//! `grow` is the Magic Wand and the Paint Bucket: similarity stays anchored at
//! the single seed, and its predicate is Grow/Similar's key form
//! (`ReferenceRange::from_seed`: R, G and B within ± tolerance of the seed's,
//! the halved cyclic differences within ± ⌊tolerance/2⌋, alpha within ±
//! tolerance; `selection_similar`'s module doc, whose **The takes** records
//! the measurement), four-connected, `≤`.
//!
//! Scaling: every depth compares bytes (`selection_similar`'s **Resolution**): R, G and B of a u16
//! sample — a 16-bit document's, a 32-bit float document's R1 display codes — read as their
//! nearest byte and the tolerance the host sends (8-bit t × 257) back to t, the halved
//! differences taken on those bytes; alpha keeps its own codes. A gray mask or channel
//! (`[v, v, v, max]`) reduces to |Δ byte| ≤ t.
//!
//! **Ink (#1).** A CMYK document's Magic Wand and Paint Bucket compare the ink's NAIVE RGB, never
//! the view: [`grow_ink`] is `grow`'s twin over straight CMYKA16 samples and the same [`flood`],
//! each sample read through [`ink_as_rgb`] — `R = (1 − C)(1 − K)` (G from M, B from Y, A the alpha),
//! a byte at the document's depth: at 8 bits `round((255 − c8)(255 − k8)/255)` with
//! `c8 = round(C/257)`; at 16 the product on the 32768-step grid, each ink
//! `i = round(v·32768/65535)` and `R15 = round((32768 − iC)(32768 − iK)/32768)`, read to a byte
//! as `⌊(R15·255 + 16384)/32768⌋` — and then `grow`'s own predicate ([`ReferenceRange`]'s keys of
//! that naive RGB), four-connected. K enters only through the darkening it shares with every
//! plate, so a K pair or a GCR twin is admitted wherever its RGB is (measured against Photoshop).
//! The naive RGB source (PN7, the seven projections of the time on it) was fitted on
//! `scripts/photoshop-probe/cmyk_select_take.py`'s take1 (0 regions mismatched of 12 cases, where
//! the shipped 11-projection ink box missed 336 and the five-plate box 136; the unrounded RGB
//! missed 16, so the 8-bit rounding is part of the rule) and confirmed by `cmyk_k_take.py`'s take;
//! the record is in `designs/cmyk-mode.md`. `wand_key_79_take.py` replaced the projections by the
//! halved keys on the same source, exact at both depths (`selection_similar`'s **The takes**).
//! The 16-bit product is
//! `scripts/photoshop-probe/key_resolution_80_take.py`'s refit of its take1 (`key-resolution-80`,
//! Photoshop 26.10.0), confirmed on its held-out take2: 0 pixels a tool, where the 16-bit product
//! on the codes read to a byte missed 16 and 14 pixels a tool, the inks' 8-bit product 98 to 154
//! and the u16 comparison 179 to 258.
//!
//! **Sample Size.** The wand's reference is the seed pixel (`sample_size` 1, Point
//! Sample, which the Paint Bucket always passes) or the per-channel truncating mean of the
//! `sample_size`-square box about it, clipped to the canvas (`ops_stats::block_mean`, the
//! eyedropper's own mean), over the keys the predicate compares (`selection_similar::keys`):
//! bytes of R, G and B — of native codes, of a float document's R1 display codes, of the ink's
//! naive RGB — and alpha's own codes. On an 8-bit typed document that is the byte wand's mean on
//! the same pixels (Lab averages its codes before its own reduction, `lab_select`); unmeasured.
//! [`WAND_SAMPLE_SIZES`] is the Sample Size list (Point, 3, 5, 11, 31, 51 and 101 by 101
//! Average). Contiguous marks the seed full whatever the averaged reference admits (`flood`'s
//! rule); global tests every pixel.
use crate::job_control::JobContext;
use crate::precision_quantize::nearest;
use crate::selection_similar::{grid15, grid15_byte, keys, ReferenceRange};
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
/// keys each lie within their bound of the reference's
/// (`ReferenceRange::admits`) — the seed's, or those of its `sample_size`
/// box mean (module doc).
pub(crate) fn grow<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    seed: (u32, u32),
    tolerance: T,
    contiguous: bool,
    sample_size: u32,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let keyed = |i: usize| keys(pixel(i));
    let range = ReferenceRange::from_seed(reference(dimensions, keyed, seed, sample_size)?);
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
/// `(1 − ink)(1 − K)` per plate as a byte at the document's depth — the 8-bit product of the
/// inks' bytes, or at 16 bits the product on the 32768-step grid read to a byte — returned on
/// the ×257 grid, so `selection_similar::keys` reads that byte back exactly; alpha as it is.
pub(crate) fn ink_as_rgb(p: [u16; 5], depth_bits: u32) -> [u16; 4] {
    let [c, m, y, k, a] = p;
    let plate = |v: u16| -> u16 {
        let byte = if depth_bits <= 8 {
            let (v8, k8) = (u32::from(nearest(v)), u32::from(nearest(k)));
            ((255 - v8) * (255 - k8) + 127) / 255
        } else {
            let (iv, ik) = (grid15(v, 32768), grid15(k, 32768));
            // round((32768 − iv)(32768 − ik) / 32768), ties up.
            let r15 = (2 * (32768 - iv) * (32768 - ik) + 32768) / 65536;
            grid15_byte(r15) as u32
        };
        (257 * byte) as u16
    };
    [plate(c), plate(m), plate(y), a]
}

/// The CMYK wand / bucket region from the single seed (module doc, **Ink**):
/// `grow`'s rule over [`ink_as_rgb`] of straight CMYKA16 samples at the
/// document's `depth_bits`, through the same [`flood`]; a `sample_size` box
/// averages those naive RGB bytes.
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
    let keyed = |i: usize| keys(ink_as_rgb(pixel(i), depth_bits));
    let range = ReferenceRange::from_seed(reference(dimensions, keyed, seed, sample_size)?);
    let start = seed.1 as usize * dimensions.0 as usize + seed.0 as usize;
    flood(
        dimensions,
        [start],
        |i| range.admits(ink_as_rgb(pixel(i), depth_bits), tolerance),
        contiguous,
        job,
    )
}
