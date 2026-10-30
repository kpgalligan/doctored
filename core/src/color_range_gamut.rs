//! Select ▸ Color Range ▸ Out Of Gamut: the pixels the gamut warning
//! flags, as selection coverage. There is no second gamut test here: the
//! "outside" bit is `soft_proof`'s one rule (`GAMUT_DELTA_E` between a
//! colour's Lab and its relative `B2A1 → A2B1` round trip, independent of the
//! setup's intent), read from `soft_proof::proof_render8_with`'s mask over the
//! same RGBA8 codes the window's gamut warning proofs — native codes at
//! 8 bits, the projection's display codes at 16 bits, the R1 display codes at
//! 32 bits. The host picks those codes; this adapter only turns the mask into
//! coverage.
//!
//! Coverage is the source pixel's alpha where `outside XOR invert`, else 0,
//! so Invert selects the in-gamut pixels and a transparent pixel is never
//! selected, the rule `selection_tools::color_range_samples` applies to every
//! other Color Range mode. Out Of Gamut is binary (no Fuzziness), and a
//! CMYK document is all in gamut, which the host answers without calling
//! here.

use image::RgbaImage;

use crate::icc::IccProfile;
use crate::icc_lut::ProofProfile;
use crate::job_control::JobContext;
use crate::soft_proof::{self, ProofSetup};

/// Out Of Gamut coverage for `flat` (module doc): one byte per pixel, the
/// source alpha where the gamut warning's rule flags the pixel (or, with
/// `invert`, where it does not), else 0. `Err` carries the proof's own
/// refusal (a profile with no model, a proof profile without both tables) or
/// `operation cancelled`.
pub(crate) fn out_of_gamut(
    flat: &RgbaImage,
    document: &IccProfile,
    proof: &ProofProfile,
    setup: &ProofSetup,
    invert: bool,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let output = soft_proof::proof_render8_with(flat, document, proof, setup, true, job)?;
    let mut coverage = output.mask.ok_or("the gamut mask was not produced")?;
    // The second pass over the source supplies alpha; the mask is 255/0.
    for (i, (slot, pixel)) in coverage.iter_mut().zip(flat.pixels()).enumerate() {
        job.every(i)?;
        let outside = *slot == 255;
        *slot = if outside != invert { pixel[3] } else { 0 };
    }
    Ok(coverage)
}
