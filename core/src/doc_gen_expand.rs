//! Generative Expand (M3b phase 2): the coverage op over the full
//! composite at 8 bits (`DrDocument`), 16-bit RGB, 32-bit float RGB and 8-bit
//! CMYK (`PrecisionDocument`). After Image ▸ Canvas Size leaves a transparent
//! frame round the picture, the host asks [`DrDocument::gen_expand_coverage`]
//! for the canvas-sized u16 coverage `gen_fill::transparency_coverage` of
//! each pixel's composite alpha (`65535 − α`), then runs Generative Fill's
//! own frame, input and apply over it (`doc_gen_fill`), so an Expand layer is
//! an ordinary Generative Layer whose mask is that coverage at (0, 0). Every
//! pixel the picture already covers has coverage 0 there, so the composite
//! keeps it bit-exact.
//!
//! The plate is the one Generative Fill's input reads: byte
//! `inpaint_source::sampling_source` with `RetouchSampling::All`; 16-bit and
//! float RGB `render_cut_with_context` with an empty cut (a float alpha is
//! clamped to 0…1 before it is quantised, so the coverage of a float pixel is
//! `65535 − round(clamp(α, 0, 1)·65535)`); 8-bit CMYK
//! `cmyk_gen_fill_plate` (alpha `s[4]`). The plan is that plate's term alone
//! (byte `4n + plate_scratch_bound`, 16-bit `gen_fill_plate_term`, CMYK
//! `cmyk_plate_scratch(All)`): the coverage buffer is the host's. Each call
//! admits and reserves it against `limit` beside `other` before it
//! allocates, and drops the plate before it returns. `layer` is only the
//! anchor's range check, as for Fill. A 16-bit CMYK document refuses by name,
//! and a picture with no transparent pixel refuses with the sentence that
//! says to enlarge the canvas first. The float arm is the coverage only
//! (phase 2b-1: Auto-Blend's Content-Aware Fill Transparent Areas reads
//! it); Generative Fill and Expand themselves stay refused at 32 bits, by
//! `doc_gen_fill`'s own depth check and the host's.

use crate::cmyk_document::CmykDepth;
use crate::doc::DrDocument;
use crate::doc_gen_fill::begin;
use crate::gen_fill::transparency_coverage;
use crate::inpaint_source::{plate_scratch_bound, sampling_source};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::retouch_brush_sampling::plate_samples;
use crate::retouch_sampling::RetouchSampling;
use crate::style_composite::Cut;

const NOT_CMYK16: &str = "generative expand is not available in a 16-bit CMYK document";
const NO_TRANSPARENCY: &str =
    "the picture has no transparent area to expand into: enlarge the canvas first";
pub(crate) const BAD_BUFFER: &str = "invalid generative expand coverage buffer";

fn out_of_range(layer: usize) -> String {
    format!("generative expand layer {layer} is out of range")
}

/// `coverage_out` holds exactly `n` samples, else the refusal.
fn coverage_length(coverage_out: &[u16], n: u64) -> Result<(), String> {
    if coverage_out.len() as u64 != n {
        return Err(BAD_BUFFER.into());
    }
    Ok(())
}

/// Writes row `y`'s coverage from its u16 `alphas` (one per pixel) and
/// answers whether any sample is in the hole.
fn write_row(
    coverage_out: &mut [u16],
    width: usize,
    y: u32,
    alphas: impl Iterator<Item = u16>,
) -> bool {
    let at = y as usize * width;
    let mut any = false;
    for (slot, alpha) in coverage_out[at..at + width].iter_mut().zip(alphas) {
        *slot = transparency_coverage(alpha);
        any |= *slot != 0;
    }
    any
}

/// Every row of a `height`-row canvas through `row`, polling `job`; the
/// no-transparency refusal when no row has a sample in the hole.
fn every_row(
    height: u32,
    job: JobContext<'_>,
    mut row: impl FnMut(u32) -> Result<bool, String>,
) -> Result<(), String> {
    let mut any = false;
    for y in 0..height {
        job.check()?;
        any |= row(y)?;
    }
    if !any {
        return Err(NO_TRANSPARENCY.into());
    }
    job.report(1.0, 5)
}

impl DrDocument {
    /// The coverage op's working set: the byte plate `4n` and
    /// `plate_scratch_bound` (module doc).
    pub(crate) fn gen_expand_coverage_plan(&self) -> Result<u64, String> {
        let n = u64::from(self.width) * u64::from(self.height);
        Ok(n.saturating_mul(4)
            .saturating_add(plate_scratch_bound(self)?))
    }

    /// Writes Generative Expand's canvas-sized coverage (module doc) into
    /// `coverage_out` (`width·height` samples). `Err` for an out-of-range
    /// anchor, a mis-sized buffer, a refused plan, a cancel and a picture
    /// with no transparent pixel.
    pub fn gen_expand_coverage(
        &self,
        layer: usize,
        coverage_out: &mut [u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        if layer >= self.layers.len() {
            return Err(out_of_range(layer));
        }
        let n = u64::from(self.width) * u64::from(self.height);
        coverage_length(coverage_out, n)?;
        let plan = self.gen_expand_coverage_plan()?;
        let (_run, job) = begin(control)?;
        admit(self.gen_fill_retained(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let plate = sampling_source(
            self,
            layer,
            RetouchSampling::All,
            false,
            n.saturating_mul(4),
            job.range(0.0, 0.8),
        )?
        .ok_or("generative expand could not read the picture")?;
        let width = self.width as usize;
        let raw = plate.as_raw();
        every_row(self.height, job.range(0.8, 1.0), |y| {
            let at = y as usize * width * 4;
            let alphas = raw[at..at + width * 4]
                .chunks_exact(4)
                .map(|p| u16::from(p[3]) * 257);
            Ok(write_row(coverage_out, width, y, alphas))
        })
    }
}

impl PrecisionDocument {
    /// A 16-bit CMYK document refuses by name (the expand twin of
    /// `doc_gen_fill`'s depth check, with Expand's sentence); a float
    /// document is admitted (module doc).
    fn gen_expand_depth(&self) -> Result<(), String> {
        if self
            .cmyk_policy()
            .is_some_and(|p| p.depth() != CmykDepth::Eight)
        {
            return Err(NOT_CMYK16.into());
        }
        Ok(())
    }

    /// The 16-bit and float twin of [`DrDocument::gen_expand_coverage_plan`]:
    /// the RGB plate term, or `cmyk_plate_scratch(All)` on 8-bit CMYK.
    pub(crate) fn gen_expand_coverage_plan(&self) -> Result<u64, String> {
        self.gen_expand_depth()?;
        if self.is_cmyk() {
            return self.cmyk_plate_scratch(RetouchSampling::All);
        }
        self.gen_fill_plate_term()
    }

    /// The 16-bit and float twin of [`DrDocument::gen_expand_coverage`]; a
    /// 16-bit CMYK document refuses by name.
    pub fn gen_expand_coverage(
        &self,
        layer: usize,
        coverage_out: &mut [u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        self.gen_expand_depth()?;
        self.layer(layer).ok_or_else(|| out_of_range(layer))?;
        let (w, h) = self.dimensions();
        coverage_length(coverage_out, u64::from(w) * u64::from(h))?;
        let plan = self.gen_expand_coverage_plan()?;
        let (_run, job) = begin(control)?;
        admit(self.retained_bytes(), plan, other, limit)?;
        job.reserve(plan)?;
        let job = job.with_reservation_floor(plan);
        let width = w as usize;
        if self.is_cmyk() {
            let plate = self.cmyk_gen_fill_plate(job.range(0.0, 0.8))?;
            return every_row(h, job.range(0.8, 1.0), |y| {
                let alphas = plate
                    .row_segments(y, 0, w)
                    .flat_map(|(_, run)| run.iter().map(|s| s[4]));
                Ok(write_row(coverage_out, width, y, alphas))
            });
        }
        let excluded = vec![false; self.records];
        let plate =
            self.render_cut_with_context(Cut::new(&excluded), limit, other, job.range(0.0, 0.8))?;
        every_row(h, job.range(0.8, 1.0), |y| {
            let samples = plate_samples(&plate, (0, y, w, 1))?;
            let alphas = samples
                .iter()
                .map(|p| (p[3].clamp(0.0, 1.0) * 65535.0).round() as u16);
            Ok(write_row(coverage_out, width, y, alphas))
        })
    }
}
