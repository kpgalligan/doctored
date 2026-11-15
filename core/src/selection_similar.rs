//! Select ▸ Grow (contiguous) and Select ▸ Similar (global): colour-based
//! growth of the current selection, NOT a pixel-radius expand (that is
//! `modify_selection` grow, Select ▸ Modify ▸ Expand).
//!
//! The predicate follows a measurement of Photoshop 26.10.0 made on
//! 2026-09-27. Every pixel the selection covers is a reference; the
//! references span a range on seven projections — R, G, B, A, R−G, G−B and
//! R−B — (trimmed on a Grayscale document, **The reference range**) and a
//! pixel is admitted when every projection lies within that range widened by
//! the tolerance on both sides. Measured facts: red and
//! blue references at tolerance 0 admit magenta, black and (100,0,100) and
//! refuse grey; a grey-100 reference at tolerance 10 admits ±10 and refuses
//! ±11; (105,95,100) is in and (106,94,100) is out of that range; grey-100
//! and grey-120 references at tolerance 0 exclude (110,100,120) and admit
//! (110,110,110). Grow is four-connected from the references. The result is
//! the admitted region at full coverage, added to the selection.
//!
//! **The reference range.** On a Grayscale document (and a Duotone one, a
//! Gray document with inks; at 32 bits by the same rule, unmeasured, on its
//! display codes) each projection's values over the N references lose
//! ⌊N/10⌋ at each end, every projection on its own ([`TRIM_DIVISOR`], the
//! rule `lab_select` measured on Lab), before the tolerance widens what is
//! left: a value only a few references carry widens nothing. RGB, Indexed,
//! float RGB and CMYK (the ink's naive RGB, PN7) keep every reference's
//! value. `scripts/photoshop-probe/similar_trim_61_take.py` (Photoshop
//! 26.10.0, `similar-trim-61`) measured them with exact membership on every
//! pixel: take1 laid `lab_select_5_take.py`'s 37 reference rectangles and
//! single-pixel probes on RGB, Gray and CMYK scenes at 8 and 16 bits, t 15
//! and 25; take2, held out, its scene B (t 13 and 27, other counts and the
//! rects that pin the divisor). Gray met the trim on byte codes on both
//! takes at both depths, ⌊N/10⌋ alone of the shares 9 %…11 % on take2, and
//! the full box missed (376 and 408 pixels at 8 bits). At 16 bits the u16
//! comparison misses only probes one code past the bound (28 pixels a take),
//! where Photoshop's 16-bit store moved a byte × 257 code by one; whether the
//! 16-bit rule compares bytes or u16 codes is not separated by these scenes
//! (#80). RGB and CMYK met neither the full box nor
//! the trim on these projections; both takes met the trim on
//! `lab_select::wand_key`'s key form instead (R, G, B ± t and the halved
//! cyclic differences ± ⌊t/2⌋, byte codes at both depths; CMYK's on the
//! naive RGB), which those documents do not ship, so they keep the full box
//! until it lands with the wand's (#61, held on #79).
//!
//! Not measured, and chosen here: the sample source is the flattened
//! composite (as the wand's "select what you see"), not the active layer;
//! alpha is one more projection rather than a special case for transparency.
//!
//! The Magic Wand and the Paint Bucket (`selection_region::grow`) run the
//! same predicate anchored at their one seed ([`ReferenceRange::from_seed`]:
//! min = max = the seed's projections), measured against Photoshop
//! 26.10.0 (`selection_region`'s module doc).
//!
//! Byte `dr_doc_select_similar` samples the flattened composite; the typed `similar` kind of
//! `dr_precision_doc_select` reads the u16 selection as its planned input.
use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::selection_region::flood;
use crate::selection_tools::SelectionSample;

/// The refusal of Grow and Similar (and the Lab forms in `lab_select`) when
/// no pixel is a reference ([`is_reference`]).
pub(crate) const NO_REFERENCE: &str =
    "No pixel is more than half selected, so there is no colour to match.";

/// The per-projection [min, max] of the reference pixels, in integer codes.
pub(crate) struct ReferenceRange {
    min: [i32; 7],
    max: [i32; 7],
}

/// R, G, B, A, R−G, G−B, R−B. The three differences are what make a hue
/// shift count double against the tolerance: (106,94,100) moves R and G by
/// only 6 from grey 100, yet R−G moves by 12 and leaves a tolerance-10 range,
/// which is what Photoshop was measured to do. A brightness shift moves all
/// three channels together and leaves the differences where they were.
fn projections<T: SelectionSample>(p: [T; 4]) -> [i32; 7] {
    let [r, g, b, a] = p.map(|c| c.code() as i32);
    [r, g, b, a, r - g, g - b, r - b]
}

/// A reference pixel is at least half selected: byte 128, u16 32768. This
/// half-way cut is a Doctored convention (Photoshop's own threshold for a
/// partly selected pixel was not measured); it matches the "more than half
/// selected" wording of the refusal ([`NO_REFERENCE`]).
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
    /// The range of the single reference `seed`: the wand's and the
    /// bucket's predicate, every projection within ± tolerance of the
    /// seed's own.
    pub(crate) fn from_seed<T: SelectionSample>(seed: [T; 4]) -> Self {
        let v = projections(seed);
        Self { min: v, max: v }
    }

    /// Similar's and Grow's range over every reference pixel
    /// ([`is_reference`]), each projection on its own: its values' [min, max],
    /// or with `trim` (module doc, **The reference range**) that range once
    /// ⌊N/[`TRIM_DIVISOR`]⌋ of the N values are dropped from EACH end. None
    /// when no pixel is a reference.
    pub(crate) fn from_reference<T: SelectionSample>(
        pixel: &impl Fn(usize) -> [T; 4],
        coverage: &[T],
        trim: bool,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        // A count per value of each projection, so the trim reads histograms
        // rather than holding every reference's projections: R, G, B and A
        // in 0…MAX, the three differences in −MAX…MAX shifted up by MAX. At
        // 16 bits that is 4·65536 + 3·131071 counts (5.2 MB), whatever the
        // canvas; at 8 bits 20 KB.
        let top = T::MAX as usize;
        let mut starts = [0usize; 8];
        for k in 0..7 {
            starts[k + 1] = starts[k] + if k < 4 { top + 1 } else { 2 * top + 1 };
        }
        let bias = |k: usize| if k < 4 { 0 } else { top as i32 };
        let mut counts = reserved_vec(starts[7])?;
        counts.resize(starts[7], 0u64);
        let mut n = 0u64;
        for (i, &c) in coverage.iter().enumerate() {
            job.every(i)?;
            if !is_reference(c) {
                continue;
            }
            for (k, v) in projections(pixel(i)).into_iter().enumerate() {
                // Always in range for T's codes; the clamp only keeps an
                // index from panicking.
                let at = (v + bias(k)).clamp(0, (starts[k + 1] - starts[k] - 1) as i32);
                counts[starts[k] + at as usize] += 1;
            }
            n += 1;
        }
        if n == 0 {
            return Ok(None);
        }
        let dropped = if trim { n / TRIM_DIVISOR } else { 0 };
        let mut min = [0; 7];
        let mut max = [0; 7];
        for k in 0..7 {
            let (low, high) = trimmed(&counts[starts[k]..starts[k + 1]], dropped);
            (min[k], max[k]) = (low - bias(k), high - bias(k));
        }
        Ok(Some(Self { min, max }))
    }

    pub(crate) fn admits<T: SelectionSample>(&self, p: [T; 4], tolerance: T) -> bool {
        let tolerance = tolerance.code() as i32;
        projections(p)
            .iter()
            .enumerate()
            .all(|(k, &v)| v >= self.min[k] - tolerance && v <= self.max[k] + tolerance)
    }
}

/// The admitted region at full coverage (the caller adds it to the current
/// selection): global, or flooded four-connected from every reference pixel
/// when `contiguous`. A partly covered pixel that is admitted comes back full.
/// `trim` is the document's kind's reference rule (module doc, **The
/// reference range**): true on a Grayscale document.
pub(crate) fn similar<T: SelectionSample>(
    dimensions: (u32, u32),
    pixel: impl Fn(usize) -> [T; 4],
    coverage: &[T],
    tolerance: T,
    contiguous: bool,
    trim: bool,
    job: JobContext<'_>,
) -> Result<Vec<T>, String> {
    let (w, h) = dimensions;
    if coverage.len() != w as usize * h as usize {
        return Err("selection mask length does not match the canvas".into());
    }
    let range = ReferenceRange::from_reference(&pixel, coverage, trim, job)?.ok_or(NO_REFERENCE)?;
    let seeds = coverage
        .iter()
        .enumerate()
        .filter(|(_, &c)| is_reference(c))
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
    /// admits from the canvas-sized coverage `mask`, every reference's value
    /// kept (an RGB document, module doc). Err with the refusal sentence when
    /// no pixel is at least half selected, or when `mask` is not canvas-sized.
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
            false,
            JobContext::NONE,
        )
    }
}
