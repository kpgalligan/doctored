//! Select ▸ Grow (contiguous) and Select ▸ Similar (global): colour-based
//! growth of the current selection, NOT a pixel-radius expand (that is
//! `modify_selection` grow, Select ▸ Modify ▸ Expand).
//!
//! Every pixel the selection covers is a reference. A pixel is compared on
//! its KEYS ([`halved_keys`], the form `lab_select::wand_key` holds over
//! L\*, a\*, b\*, here over R, G and B): the three channels, the halved
//! cyclic differences q1 = ⌊(G − R + 255)/2⌋, q2 = ⌊(B − G + 255)/2⌋ and
//! q3 = ⌊(R − B + 255)/2⌋, and alpha. It is admitted when the channels lie
//! within the references' range widened by the tolerance t on both sides,
//! q1–q3 within theirs widened by ⌊t/2⌋ ([`halved_bounds`]) and alpha
//! within its own (**Resolution**). The halving is what makes a hue shift
//! count against the tolerance while a brightness shift, which moves the
//! three channels together, leaves q1–q3 where they were: from grey 100 at
//! t 15, (114,100,100) is in and (115,100,100) is out (q3 moves 8 against a
//! bound of 7), and so is every single-channel step of exactly an odd t.
//! Grow is four-connected from the references the range admits (**Grow's
//! seeds**). The result is the admitted region at full coverage, added to the
//! selection.
//!
//! **Grow's seeds.** A reference the range does not admit (a colour the trim
//! dropped, beyond the bounds) is not made full and seeds nothing: it keeps
//! its coverage through the caller's union, as every unadmitted pixel does.
//! `lab_select`'s take (`lab_similar_60_take.py`, **Partly selected and ragged
//! references** there) measured it on Lab and on this kernel's 8-bit RGB,
//! whose take1 control also met the half-selected reference rule
//! ([`is_reference`]). That such a reference does not bridge the flood either
//! was read on Lab alone, from the jobs a stalled session finished.
//!
//! **The reference range.** Each key's values over the N references lose
//! ⌊N/10⌋ at each end, every key on its own ([`TRIM_DIVISOR`], the rule
//! `lab_select` measured on Lab), before the bounds widen what is left: a
//! value only a few references carry widens nothing. This holds on every
//! document kind; a Grayscale or Duotone pixel, a mask's or a channel's is
//! `[v, v, v, a]`, whose q1–q3 are 127 whatever v, so only v and alpha
//! decide there.
//!
//! **The takes** (Photoshop 26.10.0, exact membership on every pixel).
//! `scripts/photoshop-probe/similar_trim_61_take.py` (`similar-trim-61`)
//! measured Similar and Grow: take1 laid `lab_select_5_take.py`'s 37
//! reference rectangles and single-pixel probes on RGB, Gray and CMYK
//! scenes at 8 and 16 bits, t 15 and 25; take2, held out, its scene B (t 13
//! and 27, other counts and the rects that pin the divisor). Gray met the
//! trim on bytes on both takes at both depths, ⌊N/10⌋ alone of the shares
//! 9 %…11 % on take2. On RGB and CMYK neither the full box nor the trim met
//! on the seven projections the core then compared (R, G, B, A, R−G, G−B,
//! R−B, each within ± t); the trim on the halved keys, a refit of take1,
//! met take2 with 0 pixels off at both depths in both modes.
//! `scripts/photoshop-probe/wand_key_79_take.py` (`wand-key-79`) measured
//! the Magic Wand on RGB and CMYK and the Paint Bucket on RGB, at 8 and 16
//! bits, Contiguous off and on, t 11, 19 and 24, from fields whose cyclic
//! sums take both parities, with single-pixel probes on mixed-sign rays and
//! on each sum's ±t edge: the halved keys met every case (0 pixels), the
//! seven projections missed 674 pixels a depth on RGB and 837 on CMYK
//! (take1's wand, repeated exactly by take2, which also measured the bucket
//! cases take1 lost to the harness). `blur_wand_take.py`'s 8-bit wand and
//! bucket takes (`blur-wand-v1x`, t 0 to 100), re-scored, meet the halved
//! keys on every case too, and so do the CMYK wand's confirming take
//! (`cmyk_k_take.py`) and the 8-bit wand masks of `color_range_671_take.py`
//! (`core/tests/cmyk_select.rs`, `wand_sample_size.rs`), where the seven
//! projections left residuals. The first measurement's facts (2026-09-27)
//! hold under the halved keys: red and blue references at tolerance 0 admit
//! magenta, black and (100,0,100) and refuse grey; a grey-100 reference at
//! tolerance 10 admits ±10 and (105,95,100) and refuses ±11 and
//! (106,94,100); grey-100 and grey-120 references at tolerance 0 exclude
//! (110,100,120) and admit (110,110,110). Unmeasured, and following this
//! kernel: Indexed, float RGB's display codes, Duotone, masks and channels,
//! alpha's bound and trim, the Paint Bucket on CMYK, the Sample Size mean,
//! and the Magic Eraser and the Background Eraser, which are defined by the
//! wand's admission.
//!
//! **Resolution.** Every depth compares bytes ([`keys`]). A byte sample is
//! its own key; a u16 sample — a 16-bit document's, an 8-bit typed
//! document's ×257 codes, a float document's R1 display codes, a mask's or a
//! channel's — is read as its nearest byte, (v + 128) / 257
//! (`precision_quantize::nearest`), R, G and B alike, and so is the host's
//! u16 tolerance (8-bit t × 257, read back to t: a fractional t counts as
//! its nearest whole step). Alpha keeps its own codes and the tolerance's
//! (every take was opaque; `lab_select`'s choice). A CMYK document's naive
//! RGB is a byte at both depths (`selection_region`'s **Ink**).
//! `scripts/photoshop-probe/key_resolution_80_take.py` (Photoshop 26.10.0,
//! `key-resolution-80`, two sessions, exact membership on every pixel)
//! measured it on 16-bit Gray, neutral RGB and neutral CMYK scenes: fields
//! off the ×257 grid, a checker of two codes 200 apart within one byte, and
//! single-pixel probes at 257·(b ± m) + k on both sides of every byte edge,
//! through Similar, Grow and the wand (Point Sample, Contiguous off and on)
//! at t 15 and 25, then held out at t 13 and 27 on other fields. Bytes met
//! every case of every mode on both takes (CMYK through its 15-bit product,
//! a refit of take1 that take2 confirmed); the u16 comparison (bound 257·t)
//! missed 178 to 260 pixels a tool, no u16 bound from 257·t − 300 to
//! 257·t + 300 met, and neither did truncating bytes. Photoshop's 16-bit
//! store holds a 15-bit grid, i = round(v·32768/65535): Lab's reduction of i
//! to a byte, ⌊(i·255 + 16384)/32768⌋, and (v + 128) / 257 part on 127 of
//! the 65536 u16 codes and on none the grid writes back, so the takes cannot
//! tell them apart and the nearest byte ships. An 8-bit typed document's
//! codes sit on the ×257 grid, where the reading is exact; a code paint left
//! off it reads as its nearest byte. Unmeasured, and following this
//! kernel: the Paint Bucket off the ×257 grid, masks and channels, Duotone,
//! float RGB's display codes, alpha, and a Sample Size box, whose mean is
//! taken over the bytes before they are keyed (`selection_region`'s
//! **Sample Size**).
//!
//! Not measured, and chosen here: the sample source is the flattened
//! composite (as the wand's "select what you see"), not the active layer;
//! alpha is one more key rather than a special case for transparency.
//!
//! The Magic Wand and the Paint Bucket (`selection_region::grow`) run the
//! same predicate anchored at their one seed ([`ReferenceRange::from_seed`]:
//! min = max = the seed's keys), measured by `wand-key-79` (**The takes**).
//!
//! Byte `dr_doc_select_similar` samples the flattened composite; the typed `similar` kind of
//! `dr_precision_doc_select` reads the u16 selection as its planned input.
use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::precision_quantize::nearest;
use crate::selection_region::flood;
use crate::selection_tools::SelectionSample;

/// The refusal of Grow and Similar (and the Lab forms in `lab_select`) when
/// no pixel is a reference ([`is_reference`]).
pub(crate) const NO_REFERENCE: &str =
    "No pixel is more than half selected, so there is no colour to match.";

/// The per-key [min, max] of the reference pixels' [`projections`].
pub(crate) struct ReferenceRange {
    min: [i32; 7],
    max: [i32; 7],
}

/// The colour keys of one byte triple (module doc): the three channels and
/// the halved cyclic differences ⌊(c1 − c0 + 255)/2⌋, ⌊(c2 − c1 + 255)/2⌋,
/// ⌊(c0 − c2 + 255)/2⌋. The ONE implementation: RGB's, the naive RGB of a
/// CMYK ink, and Lab's L8, a8, b8 (`lab_select::wand_key`). Each sum lies in
/// 0…510, so `/ 2` is the floor and every key lies in 0…255.
pub(crate) fn halved_keys([c0, c1, c2]: [i32; 3]) -> [i32; 6] {
    [
        c0,
        c1,
        c2,
        (c1 - c0 + 255) / 2,
        (c2 - c1 + 255) / 2,
        (c0 - c2 + 255) / 2,
    ]
}

/// The bounds of [`halved_keys`] for a byte tolerance `t`: ± t on the three
/// channels, ± ⌊t/2⌋ on the halved differences (module doc).
pub(crate) fn halved_bounds(t: i32) -> [i32; 6] {
    [t, t, t, t / 2, t / 2, t / 2]
}

/// One colour code as the byte the predicate compares (module doc,
/// **Resolution**): a byte is its own; a u16 code is its nearest byte,
/// `precision_quantize::nearest`. Widening a byte by ×257 first makes both
/// one expression, exact for every byte.
pub(crate) fn byte_key<T: SelectionSample>(c: T) -> i32 {
    let wide = c.code() as u64 * (65535 / T::MAX);
    i32::from(nearest(wide as u16))
}

/// The codes a pixel is compared on (module doc, **Resolution**): R, G and
/// B as bytes ([`byte_key`]), alpha in its own codes. The wand's Sample Size
/// box averages these (`selection_region::reference`).
pub(crate) fn keys<T: SelectionSample>(p: [T; 4]) -> [u16; 4] {
    let [r, g, b] = [p[0], p[1], p[2]].map(|c| byte_key(c) as u16);
    [r, g, b, p[3].code() as u16]
}

/// The keys a pixel is compared on, from its [`keys`]: [`halved_keys`] of
/// R, G and B, then alpha in its own codes (module doc).
fn projections(keys: [u16; 4]) -> [i32; 7] {
    let [r, g, b, a] = keys.map(i32::from);
    let [c0, c1, c2, q1, q2, q3] = halved_keys([r, g, b]);
    [c0, c1, c2, q1, q2, q3, a]
}

/// The index of alpha in [`projections`].
const ALPHA: usize = 6;

/// One u16 code on the 15-bit grid of `steps` steps, round(v · steps /
/// 65535) with ties up (module doc, **Resolution**: the grid Photoshop's
/// 16-bit store holds; `lab_select`'s wand and a 16-bit CMYK plate read it).
pub(crate) fn grid15(v: u16, steps: u64) -> u64 {
    (2 * u64::from(v) * steps + 65535) / 131070
}

/// A code of the 32768-step grid as its nearest byte, ⌊(i·255 + 16384) /
/// 32768⌋ (Lab's 16-bit wand byte; a 16-bit CMYK plate's).
pub(crate) fn grid15_byte(i: u64) -> i32 {
    ((i * 255 + 16384) / 32768) as i32
}

/// A reference pixel is at least half selected: byte 128, u16 32768, each
/// counting one in N whatever its coverage. `lab_similar_60_take.py`'s take1
/// measured the cut on Lab at 8 and 16 bits and on 8-bit RGB: a tail at
/// coverage 128 counts and one at 127 does not, and no coverage-weighted count
/// met (`lab_select`'s module doc). It matches the "more than half selected"
/// wording of the refusal ([`NO_REFERENCE`]).
pub(crate) fn is_reference<T: SelectionSample>(coverage: T) -> bool {
    coverage.code() >= T::MAX.div_ceil(2) as f32
}

/// Similar's and Grow's trim (module doc, **The reference range**, and
/// `lab_select`'s, **Similar and Grow**): of N references, each key drops
/// its ⌊N/10⌋ lowest and ⌊N/10⌋ highest values. The takes' scene-B rects pin
/// the divisor: a 60-value tail of 600 references is dropped and a 109-value
/// tail of 1089 kept, which no other share between 9.77 % and 10.06 % (Lab's
/// take1 interval) does.
pub(crate) const TRIM_DIVISOR: u64 = 10;

/// The smallest and largest value of histogram `counts` once `trim` values
/// are dropped from each end. `trim` is under half the total
/// (⌊N/10⌋ ≤ ⌊(N − 1)/2⌋ for every N ≥ 1), so a value is left at each end.
pub(crate) fn trimmed(counts: &[u64], trim: u64) -> (i32, i32) {
    let mut seen = 0;
    let mut low = 0;
    for (v, &c) in counts.iter().enumerate() {
        seen += c;
        if seen > trim {
            low = v;
            break;
        }
    }
    seen = 0;
    let mut high = low;
    for (v, &c) in counts.iter().enumerate().rev() {
        seen += c;
        if seen > trim {
            high = v;
            break;
        }
    }
    (low as i32, high as i32)
}

impl ReferenceRange {
    /// The range of the single reference whose [`keys`] are `seed` (a
    /// pixel's, or a Sample Size box's mean of them): the wand's and the
    /// bucket's predicate, every key within its bound of the seed's own.
    pub(crate) fn from_seed(seed: [u16; 4]) -> Self {
        let v = projections(seed);
        Self { min: v, max: v }
    }

    /// Similar's and Grow's range over every reference pixel
    /// ([`is_reference`]), each key on its own: its values' [min, max] once
    /// ⌊N/[`TRIM_DIVISOR`]⌋ of the N values are dropped from EACH end
    /// (module doc, **The reference range**). None when no pixel is a
    /// reference.
    pub(crate) fn from_reference<T: SelectionSample>(
        pixel: &impl Fn(usize) -> [T; 4],
        coverage: &[T],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // A count per value of each key, so the trim reads histograms rather
        // than holding every reference's keys: the six colour keys in 0…255
        // ([`halved_keys`]), alpha in 0…MAX. At 16 bits that is 6·256 +
        // 65536 counts (0.54 MB, the alpha histogram as Lab's), whatever the
        // canvas; at 8 bits 14 KB.
        let size = |k: usize| if k == ALPHA { T::MAX as usize + 1 } else { 256 };
        let mut starts = [0usize; 8];
        for k in 0..7 {
            starts[k + 1] = starts[k] + size(k);
        }
        let mut counts = reserved_vec(starts[7])?;
        counts.resize(starts[7], 0u64);
        let mut n = 0u64;
        for (i, &c) in coverage.iter().enumerate() {
            job.every(i)?;
            if !is_reference(c) {
                continue;
            }
            for (k, v) in projections(keys(pixel(i))).into_iter().enumerate() {
                // Always in range for T's codes; the clamp only keeps an
                // index from panicking.
                let at = v.clamp(0, (size(k) - 1) as i32);
                counts[starts[k] + at as usize] += 1;
            }
            n += 1;
        }
        if n == 0 {
            return Ok(None);
        }
        let dropped = n / TRIM_DIVISOR;
        let mut min = [0; 7];
        let mut max = [0; 7];
        for k in 0..7 {
            (min[k], max[k]) = trimmed(&counts[starts[k]..starts[k + 1]], dropped);
        }
        Ok(Some(Self { min, max }))
    }

    /// True when every key of `p`'s [`keys`] lies within the range widened
    /// by its bound: [`halved_bounds`] of `tolerance` read as a byte
    /// ([`byte_key`]) on the six colour keys, `tolerance` itself on alpha
    /// (module doc, **Resolution**).
    pub(crate) fn admits<T: SelectionSample>(&self, p: [T; 4], tolerance: T) -> bool {
        let colour = halved_bounds(byte_key(tolerance));
        let alpha = tolerance.code() as i32;
        projections(keys(p)).iter().enumerate().all(|(k, &v)| {
            let bound = if k == ALPHA { alpha } else { colour[k] };
            v >= self.min[k] - bound && v <= self.max[k] + bound
        })
    }
}

/// The admitted region at full coverage (the caller adds it to the current
/// selection): global, or flooded four-connected from every reference pixel
/// the range admits when `contiguous` (module doc, **Grow's seeds**). A partly
/// covered pixel that is admitted comes back full; one that is not, a
/// reference included, keeps its own coverage through the caller's union.
pub(crate) fn similar<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    coverage: &[T],
    tolerance: T,
    contiguous: bool,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let (w, h) = dimensions;
    if coverage.len() != w as usize * h as usize {
        return Err("selection mask length does not match the canvas".into());
    }
    let range = ReferenceRange::from_reference(&pixel, coverage, job)?.ok_or(NO_REFERENCE)?;
    let seeds = coverage
        .iter()
        .enumerate()
        .filter(|&(i, &c)| is_reference(c) && range.admits(pixel(i), tolerance))
        .map(|(i, _)| i);
    flood(
        dimensions,
        seeds,
        |i| range.admits(pixel(i), tolerance),
        contiguous,
        job,
    )
}

impl DrDocument {
    /// Select ▸ Grow / Similar on the flattened composite ("select what you
    /// see", like `magic_wand`): the canvas-sized 0/255 region `similar`
    /// admits from the canvas-sized coverage `mask` (module doc). Err with
    /// the refusal sentence when no pixel is at least half selected, or when
    /// `mask` is not canvas-sized.
    pub fn select_similar(
        &self,
        mask: &[u8],
        tolerance: u8,
        contiguous: bool,
    ) -> Result<Vec<u8>, String> {
        let composite = self.flattened()?;
        let raw = composite.as_raw();
        similar(
            composite.dimensions(),
            |i| [raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]],
            mask,
            tolerance,
            contiguous,
            JobContext::NONE,
        )
    }
}
