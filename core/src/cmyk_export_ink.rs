//! The kept-CMYK export's stages (#547, round 2): what a profile-`document`
//! JPEG or TIFF of a CMYK document does between the composite and the
//! encode.
//!
//! * **Unstaged** — output size = canvas, no active sharpen and no spot
//!   channel: the CMYK save's own samples (`cmyk_io::cmyk_flat_samples`), so
//!   a default request is still the save's bytes.
//! * **Staged** — the composite flattened on paper (`cmyk_stack` rows through
//!   `on_paper` and `codes`, alpha 1, each spot channel overprinted in ink
//!   as its row is laid, `spot_cmyk`) as a [`CmykImage`]; resampled by
//!   `precision_sampling::resize_cmyk` (4b's CMYK geometry resampler) at the
//!   request's filter; output-sharpened by [`sharpen_ink`]; then the depth
//!   step. Sharpening follows the resample and runs on the 16-bit codes
//!   before the depth step, the RGB typed chain's order (`doc_export`).
//!
//! The depth step is the save's: 16-bit codes as they are, 8-bit
//! `(v + 128) / 257` (`cmyk_io`), never dithered.
//!
//! The staged plan follows what `cmyk_kept_samples` holds alive at once. Its
//! stages run one after another and each drops its input before the next
//! begins, so the plan is the LARGEST stage peak, not their sum (#547: the
//! sum planned 2.2–3.0× the measured Release peak), plus the export's own
//! result bytes (out × `KEEP_BYTES_PER_PIXEL`), held beside all of them:
//!
//! * on paper — the stack and one accumulator row (`cmyk_stack_scratch_bytes
//!   + w×20 + BOOKKEEPING_BYTES`) beside the flat image it fills (canvas ×
//!   `CMYK_BYTES_PER_PIXEL`);
//! * resize (only when the size changes; a same-size resample shares the
//!   flat image's tiles) — the flat image, the resampler's contiguous source
//!   copy, its output and the larger of its separable temporary and the
//!   output's adoption copy (`resize_cost_channels` over five planes);
//! * sharpen — first the resized ink (out × `CMYK_BYTES_PER_PIXEL`) beside
//!   the two RGBA16 pass inputs (out × 16), then, the ink dropped, the two
//!   passes' RGBA16 outputs (out × 16) beside `OutputSharpen::plan_bytes` and
//!   the joined ink;
//! * samples — the ink beside the interleaved samples (out × 8, one u16 slot
//!   per plate at either depth);
//! * encode — the samples beside the encode as a save charges it (out ×
//!   `SAVE_BYTES_PER_PIXEL`).
//!
//! The unstaged plan is the save's plus canvas × `KEEP_BYTES_PER_PIXEL`, as
//! round 1's was.

use crate::cmyk_composite::{codes, on_paper};
use crate::cmyk_document::CmykDepth;
use crate::cmyk_export::KEEP_BYTES_PER_PIXEL;
use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::cmyk_io::SAVE_BYTES_PER_PIXEL;
use crate::doc_export::{output_dimensions, ExportRequest};
use crate::export_sharpen::OutputSharpen;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_sampling::{
    cmyk_adoption_copy_bytes, cmyk_source_copy_bytes, resize_cmyk, resize_cost_channels,
};

/// The request's sharpen when it would change something (`is_bypass` is
/// the RGB stage's own no-op test).
fn active_sharpen(request: &ExportRequest) -> Option<OutputSharpen> {
    request.sharpen.filter(|sharpen| !sharpen.is_bypass())
}

/// PLATE-L output sharpening of opaque ink. The split is `cmyk_filter`'s
/// (its module doc is the master): pass A `[65535−C, 65535−M, 65535−Y,
/// 65535]`, pass B `[65535−K; 3, 65535]`, each an sRGB-tagged RGBA16
/// `PrecisionImage` (the numbers are `L = 1 − ink`, not a colour) through
/// `OutputSharpen::wide`, the RGB export's own 16-bit stage; C/M/Y = 65535 −
/// A's R/G/B, K = 65535 − B's G, alpha 65535.
pub(crate) fn sharpen_ink(
    image: CmykImage,
    sharpen: OutputSharpen,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let (w, h) = image.dimensions();
    let n = pixel_count(w, h)?;
    let mut a_px: Vec<[u16; 4]> = reserved_vec(n)?;
    let mut b_px: Vec<[u16; 4]> = reserved_vec(n)?;
    for y in 0..h {
        job.check()?;
        for (_, run) in image.row_segments(y, 0, w) {
            for s in run {
                a_px.push([65535 - s[0], 65535 - s[1], 65535 - s[2], 65535]);
                b_px.push([65535 - s[3], 65535 - s[3], 65535 - s[3], 65535]);
            }
        }
    }
    drop(image);
    let pass = |pixels: Vec<[u16; 4]>, part: JobContext<'_>| {
        let image = PrecisionImage::from_rgba16_unseeded(w, h, pixels, IccProfile::srgb(), None)?;
        sharpen.wide(image, part)
    };
    let a = pass(a_px, job.range(0.0, 0.45))?;
    let b = pass(b_px, job.range(0.45, 0.9))?;
    CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            for (x, dst) in (0u32..).zip(row.iter_mut()) {
                let pa = a
                    .rgba16_at(x, y)
                    .ok_or("the kept-CMYK sharpen lost a pixel")?;
                let pb = b
                    .rgba16_at(x, y)
                    .ok_or("the kept-CMYK sharpen lost a pixel")?;
                *dst = [
                    65535 - pa[0],
                    65535 - pa[1],
                    65535 - pa[2],
                    65535 - pb[1],
                    65535,
                ];
            }
            job.every(y as usize * w as usize)
        },
        job.range(0.9, 1.0),
    )
}

impl PrecisionDocument {
    /// The composite flattened on paper as opaque ink (module doc), with
    /// `finish(y, row)` run on each row once it is on paper: the spot
    /// channels' overprint (`spot_cmyk::cmyk_spot_on_paper`, the one caller),
    /// so it costs no second plane.
    pub(crate) fn cmyk_on_paper(
        &self,
        job: JobContext<'_>,
        mut finish: impl FnMut(u32, &mut [CmykSample]) -> Result<(), String>,
    ) -> Result<CmykImage, String> {
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                stack.row(i64::from(y), 0, &mut acc);
                for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                    let paper = on_paper(*ink);
                    *dst = codes([paper[0], paper[1], paper[2], paper[3], 1.0]);
                }
                finish(y, row)?;
                job.every(y as usize * w as usize)
            },
            job,
        )
    }

    /// `ink`'s interleaved C, M, Y, K samples at `depth` (8-bit: the save's
    /// `(v + 128) / 257`, never dithered; one code a u16 slot, as
    /// `cmyk_flat_samples`).
    pub(crate) fn cmyk_interleaved(
        ink: &CmykImage,
        depth: CmykDepth,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let (w, h) = ink.dimensions();
        let mut samples = reserved_vec(pixel_count(w, h)? * 4)?;
        for y in 0..h {
            job.check()?;
            for (_, run) in ink.row_segments(y, 0, w) {
                for s in run {
                    for v in &s[..4] {
                        samples.push(match depth {
                            CmykDepth::Sixteen => *v,
                            // The save's 16 → 8 (`cmyk_io`), never dithered.
                            CmykDepth::Eight => ((u32::from(*v) + 128) / 257) as u16,
                        });
                    }
                }
            }
        }
        Ok(samples)
    }

    /// Output dimensions and interleaved C,M,Y,K samples at `depth` (8-bit:
    /// one code per u16 slot, as `cmyk_flat_samples`), through the stages
    /// the request asks for (module doc). Admission belongs to the caller.
    pub(crate) fn cmyk_kept_samples(
        &self,
        request: &ExportRequest,
        depth: CmykDepth,
        job: JobContext<'_>,
    ) -> Result<((u32, u32), Vec<u16>), String> {
        let canvas = self.dimensions();
        let out = output_dimensions(canvas, request)?;
        let sharpen = active_sharpen(request);
        // Spot channels are overprinted on paper (`spot_cmyk`), so a
        // document holding one always takes the staged branch.
        let spots = self.spot_indices();
        if out == canvas && sharpen.is_none() && spots.is_empty() {
            return Ok((canvas, self.cmyk_flat_samples(depth, job)?));
        }
        let flat = self.cmyk_spot_on_paper(&spots, job.range(0.0, 0.3))?;
        let resized = resize_cmyk(
            &flat,
            out.0,
            out.1,
            request.geometry_filter(),
            job.range(0.3, 0.6),
        )?;
        drop(flat);
        let ink = match sharpen {
            Some(sharpen) => sharpen_ink(resized, sharpen, job.range(0.6, 0.9))?,
            None => resized,
        };
        Ok((out, Self::cmyk_interleaved(&ink, depth, job)?))
    }

    /// The kept branch's `scratch_bytes` (module doc).
    pub(crate) fn cmyk_kept_scratch_bytes(&self, request: &ExportRequest) -> Result<u64, String> {
        let canvas_dims = self.dimensions();
        let out_dims = output_dimensions(canvas_dims, request)?;
        let canvas = pixel_count(canvas_dims.0, canvas_dims.1)? as u64;
        let out = pixel_count(out_dims.0, out_dims.1)? as u64;
        let sharpen = active_sharpen(request);
        // A document with spot channels takes the staged branch, whose
        // on-paper image the overprint fills in place (`spot_cmyk`).
        if out_dims == canvas_dims && sharpen.is_none() && !self.has_spot_channel() {
            return Ok(self.cmyk_save_scratch_bytes()? + canvas * KEEP_BYTES_PER_PIXEL);
        }
        let flat = canvas * CMYK_BYTES_PER_PIXEL;
        let ink = out * CMYK_BYTES_PER_PIXEL;
        let samples = out * 8;
        let on_paper = self.cmyk_stack_scratch_bytes()?
            + u64::from(canvas_dims.0) * 20
            + BOOKKEEPING_BYTES
            + flat;
        let mut peak = on_paper;
        if out_dims != canvas_dims {
            let (output, temporary, _) =
                resize_cost_channels(canvas_dims, out_dims, request.geometry_filter(), 5)?;
            let resize = flat
                + cmyk_source_copy_bytes(canvas_dims)?
                + output
                + temporary.max(cmyk_adoption_copy_bytes(out_dims)?);
            peak = peak.max(resize);
        }
        if let Some(sharpen) = sharpen {
            let split = ink + out * 16;
            let passes = out * 16 + sharpen.plan_bytes(out_dims)? + ink;
            peak = peak.max(split).max(passes);
        }
        peak = peak
            .max(ink + samples)
            .max(samples + out * SAVE_BYTES_PER_PIXEL);
        Ok(peak + out * KEEP_BYTES_PER_PIXEL)
    }
}
