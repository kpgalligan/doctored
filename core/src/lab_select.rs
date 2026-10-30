//! The Lab Color document's colour-selection rules (#547 phase 4b-ii,
//! #683): how sampled Color Range measures a pixel's distance from a sample,
//! and how the Magic Wand, the Paint Bucket and Select ▸ Similar and Grow
//! admit a pixel, on a Lab document's codes. A kernel: it names no document
//! type.
//!
//! **The take.** `scripts/photoshop-probe/lab_select_take.py`'s take1
//! (Photoshop 26.10.0, `lab-select-547/take1`, 8 and 16 bits) measured
//! sampled Color Range at Fuzziness 20, 50 and 100 on a scene of cells
//! stepped in L\*, a\* and b\* about a sampled field. The declared candidates
//! were the per-channel distance on the stored codes (C1), the per-channel
//! distance on L\*/a\*/b\* units (C2), ΔE76 (C3) and Grow/Similar's seven
//! projections on the codes (C4); a candidate met a case when its distance
//! from the sample separated every selected region from every unselected one.
//! C2 alone met all six cases (the ladders put a diagonal step of v units in
//! all three channels beside an a\* step of v, which separate C2 from ΔE76),
//! so Color Range ships [`LabRule::Units`]: the largest of |ΔL\*|, |Δa\*| and
//! |Δb\*|, compared with the tolerance in those same units.
//!
//! **The wand (#683).** `scripts/photoshop-probe/lab_select_682_take.py`
//! re-scored take1's wand in place against eleven candidates and took a new
//! scene (field (60, −12, 18), t 15 and 25, 8 and 16 bits; `lab-select-682`)
//! with Select ▸ Similar and Grow run by `executeAction` of their events. The
//! gate was exact membership on every pixel. [`wand_key`] is the fit: three
//! 8-bit channel codes L8, a8, b8 and the halved cyclic differences q1 =
//! ⌊(a8 − L8 + 255)/2⌋, q2 = ⌊(b8 − a8 + 255)/2⌋, q3 = ⌊(L8 − b8 + 255)/2⌋,
//! a pixel admitted when the channels lie within ± t of the reference's and
//! q1–q3 within ± ⌊t/2⌋ (C6 at 8 bits, C9 at 16). The bytes are the rounded
//! stored byte on an 8-bit document, and on a 16-bit one Photoshop's 15-bit
//! Lab grid (L\* 32768 v/65535, a\*/b\* 32640 v/65535, rounded half up)
//! reduced to bytes, ⌊(i·255 + 16384)/32768⌋ — the 16-bit wand compares at
//! 8-bit resolution, which no form on the u16 codes matched (the best missed
//! 256 pixels). Both takes meet it exactly at both depths, for the wand and
//! (16 bits) for Similar and Grow. t is the host's tolerance read back from
//! its ×257 form; alpha is a seventh bound in u16 codes (RGB's choice, not
//! measured). Similar's references are every pixel at least half selected
//! (`selection_similar`'s rule), their keys' box widened by the same bounds;
//! Grow floods that admission four-connected from them. At 8 bits Similar and
//! Grow fit nothing (the box over a dithered 8-bit field admits a code more
//! than Photoshop does), so they refuse there (`lab_document`).

use crate::job_control::JobContext;
use crate::lab_blend::LabGrid;
use crate::selection_region::{flood, reference};
use crate::selection_similar::{is_reference, NO_REFERENCE};

/// A Lab colour-distance rule (module doc). Only the shipped one exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabRule {
    /// The largest per-channel difference in L\*, a\* and b\* units (take1's C2).
    Units,
}

/// Sampled Color Range's rule on a Lab document (module doc, **The take**).
pub(crate) const RANGE_RULE: LabRule = LabRule::Units;

/// L\*, a\*, b\* of one stored sample: the ICC v4 16-bit Lab encoding
/// (`lab_document`'s module doc, the master), L\* = 100·v/65535 and a\*,
/// b\* = 255·v/65535 − 128. Alpha is ignored.
pub(crate) fn units(p: [u16; 4]) -> [f32; 3] {
    let unit = |v: u16| f32::from(v) / 65535.0;
    [
        100.0 * unit(p[0]),
        255.0 * unit(p[1]) - 128.0,
        255.0 * unit(p[2]) - 128.0,
    ]
}

/// The inverse of [`units`]: L\* clamped to 0…100 and a\*/b\* to −128…127,
/// each rounded to its nearest u16 code (a non-finite unit reads as 0).
pub(crate) fn codes(lab: [f32; 3]) -> [u16; 3] {
    let code = |unit: f32| {
        let unit = if unit.is_finite() { unit } else { 0.0 };
        (unit.clamp(0.0, 1.0) * 65535.0).round() as u16
    };
    [
        code(lab[0].clamp(0.0, 100.0) / 100.0),
        code((lab[1].clamp(-128.0, 127.0) + 128.0) / 255.0),
        code((lab[2].clamp(-128.0, 127.0) + 128.0) / 255.0),
    ]
}

/// The distance between two L\*a\*b\* colours by `rule`, in L\*a\*b\* units.
pub(crate) fn distance(rule: LabRule, a: [f32; 3], b: [f32; 3]) -> f32 {
    match rule {
        LabRule::Units => a
            .into_iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max),
    }
}

/// One stored code as the wand's 8-bit Lab byte on `grid` (module doc, **The
/// wand**): the nearest byte of the ×257 grid at 8 bits; at 16 Photoshop's
/// 15-bit grid (L\* channel 0 at 32768 steps, a\*/b\* at 32640), rounded half
/// up, then reduced to a byte with its rounding offset 16384.
fn wand_byte(v: u16, channel: usize, grid: LabGrid) -> i32 {
    match grid {
        LabGrid::Eight => (i32::from(v) + 128) / 257,
        LabGrid::Sixteen => {
            let steps: u64 = if channel == 0 { 32768 } else { 32640 };
            let fifteen = (2 * u64::from(v) * steps + 65535) / 131070;
            ((fifteen * 255 + 16384) / 32768) as i32
        }
    }
}

/// The wand's key of one Lab sample (module doc, **The wand**): L8, a8, b8,
/// the halved cyclic differences q1, q2, q3 and the u16 alpha.
pub(crate) fn wand_key(p: [u16; 4], grid: LabGrid) -> [i32; 7] {
    let [l, a, b] = [0, 1, 2].map(|c| wand_byte(p[c], c, grid));
    // Each sum lies in 0…510, so `/ 2` is the floor the take fitted.
    [
        l,
        a,
        b,
        (a - l + 255) / 2,
        (b - a + 255) / 2,
        (l - b + 255) / 2,
        i32::from(p[3]),
    ]
}

/// The per-key bounds of a u16 `tolerance` (module doc): t = the host's
/// ×257 tolerance read back to 0…255 by rounding, ± t on the three channels,
/// ± ⌊t/2⌋ on q1–q3, and the u16 tolerance itself on alpha.
fn bounds(tolerance: u16) -> [i32; 7] {
    let t = (i32::from(tolerance) + 128) / 257;
    [t, t, t, t / 2, t / 2, t / 2, i32::from(tolerance)]
}

/// The [min, max] of the references' wand keys (module doc): one seed for the
/// wand and the bucket, every at-least-half-selected pixel for Similar and
/// Grow.
pub(crate) struct LabWandRange {
    min: [i32; 7],
    max: [i32; 7],
}

impl LabWandRange {
    /// The range of the single reference `seed`.
    pub(crate) fn from_seed(seed: [u16; 4], grid: LabGrid) -> Self {
        let key = wand_key(seed, grid);
        Self { min: key, max: key }
    }

    /// The box of every reference pixel's key (`selection_similar`'s
    /// half-selected rule); `None` when no pixel is a reference.
    pub(crate) fn from_reference(
        pixel: &impl Fn(usize) -> [u16; 4],
        coverage: &[u16],
        grid: LabGrid,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let mut range: Option<Self> = None;
        for (i, &c) in coverage.iter().enumerate() {
            job.every(i)?;
            if !is_reference(c) {
                continue;
            }
            let key = wand_key(pixel(i), grid);
            match &mut range {
                None => range = Some(Self { min: key, max: key }),
                Some(r) => {
                    for (k, &v) in key.iter().enumerate() {
                        r.min[k] = r.min[k].min(v);
                        r.max[k] = r.max[k].max(v);
                    }
                }
            }
        }
        Ok(range)
    }

    /// True when every key of `p` lies within the range widened by
    /// [`bounds`] of `tolerance`.
    pub(crate) fn admits(&self, p: [u16; 4], tolerance: u16, grid: LabGrid) -> bool {
        let bound = bounds(tolerance);
        wand_key(p, grid)
            .iter()
            .enumerate()
            .all(|(k, &v)| v >= self.min[k] - bound[k] && v <= self.max[k] + bound[k])
    }
}

/// The Lab wand / bucket region from the single seed (module doc): the
/// reference is the seed or its `sample_size` box mean over the u16 codes
/// (`selection_region::reference`), then [`LabWandRange::from_seed`] through
/// `selection_region::flood`, global or contiguous.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grow(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [u16; 4],
    seed: (u32, u32),
    tolerance: u16,
    contiguous: bool,
    sample_size: u32,
    grid: LabGrid,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let range = LabWandRange::from_seed(reference(dimensions, &pixel, seed, sample_size)?, grid);
    let start = seed.1 as usize * dimensions.0 as usize + seed.0 as usize;
    flood(
        dimensions,
        [start],
        |i| range.admits(pixel(i), tolerance, grid),
        contiguous,
        job,
    )
}

/// Select ▸ Similar (global) or Grow (`contiguous`, four-connected from every
/// reference) on Lab codes (module doc): `selection_similar::similar`'s
/// semantics over [`LabWandRange::from_reference`]. Err with that module's
/// refusal when no pixel is at least half selected, or when `coverage` is not
/// canvas-sized.
pub(crate) fn similar(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [u16; 4],
    coverage: &[u16],
    tolerance: u16,
    contiguous: bool,
    grid: LabGrid,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let (w, h) = dimensions;
    if coverage.len() != w as usize * h as usize {
        return Err("selection mask length does not match the canvas".into());
    }
    let range = LabWandRange::from_reference(&pixel, coverage, grid, job)?.ok_or(NO_REFERENCE)?;
    let seeds = coverage
        .iter()
        .enumerate()
        .filter(|(_, &c)| is_reference(c))
        .map(|(i, _)| i);
    flood(
        dimensions,
        seeds,
        |i| range.admits(pixel(i), tolerance, grid),
        contiguous,
        job,
    )
}
