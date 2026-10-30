//! Select ▸ Grow (contiguous) and Select ▸ Similar (global): colour-based
//! growth of the current selection, NOT a pixel-radius expand (that is
//! `modify_selection` grow, Select ▸ Modify ▸ Expand).
//!
//! The predicate follows a measurement of Photoshop 26.10.0 made on
//! 2026-09-27. Every pixel the selection covers is a reference; the
//! references span a range on seven projections — R, G, B, A, R−G, G−B and
//! R−B — and a pixel is admitted when every projection lies within that
//! range widened by the tolerance on both sides. Measured facts: red and
//! blue references at tolerance 0 admit magenta, black and (100,0,100) and
//! refuse grey; a grey-100 reference at tolerance 10 admits ±10 and refuses
//! ±11; (105,95,100) is in and (106,94,100) is out of that range; grey-100
//! and grey-120 references at tolerance 0 exclude (110,100,120) and admit
//! (110,110,110). Grow is four-connected from the references. The result is
//! the admitted region at full coverage, added to the selection.
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

impl ReferenceRange {
    /// The range of the single reference `seed`: the wand's and the
    /// bucket's predicate, every projection within ± tolerance of the
    /// seed's own.
    pub(crate) fn from_seed<T: SelectionSample>(seed: [T; 4]) -> Self {
        let v = projections(seed);
        Self { min: v, max: v }
    }

    /// None when no pixel is a reference.
    pub(crate) fn from_reference<T: SelectionSample>(
        pixel: &impl Fn(usize) -> [T; 4],
        coverage: &[T],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let mut range: Option<Self> = None;
        for (i, &c) in coverage.iter().enumerate() {
            job.every(i)?;
            if !is_reference(c) {
                continue;
            }
            let v = projections(pixel(i));
            match &mut range {
                None => range = Some(Self { min: v, max: v }),
                Some(r) => {
                    for (k, &p) in v.iter().enumerate() {
                        r.min[k] = r.min[k].min(p);
                        r.max[k] = r.max[k].max(p);
                    }
                }
            }
        }
        Ok(range)
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
    /// admits from the canvas-sized coverage `mask`. Err with the refusal
    /// sentence when no pixel is at least half selected, or when `mask` is
    /// not canvas-sized.
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
