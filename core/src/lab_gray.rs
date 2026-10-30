//! Image ▸ Mode ▸ Grayscale from a Lab Color document (#1 Lab phase
//! 5a-ii): the Lab arm's pixel rule, its blocker and its plan term.
//! `gray_convert`'s RGB → Gray body runs it, so the record tree, the
//! styles, the comps, the depth and the flatten choice are that body's.
//!
//! # The rule
//!
//! Each pixel's L\* alone: `Y = L*⁻¹(L*)` (D50, Yₙ = 1; `icc_lut::lab_to_xyz`
//! of `(L*, 0, 0)`, whose `Y` does not depend on a\* or b\*), then the Gray
//! profile's tone curve and the declared depth (`GrayPolicy::encode` and
//! `code`, the ×257 grid at 8 bits), written R = G = B with alpha kept by
//! `gray_convert::gray_rows_u16`. a\* and b\* are discarded; two pixels of
//! equal L\* give the same code.
//!
//! # The take
//!
//! `scripts/photoshop-probe/lab_gray_take.py` (receipts `lab-gray-547/smoke1`
//! and `take1`, 2026-10-22, Photoshop 26.10.0): Photoshop's own Lab of
//! `gray-mode-547`'s RGB-authored scene, converted by Image ▸ Mode ▸
//! Grayscale under the working Gray "Dot Gain 20%", read pixel by pixel. The
//! declared candidates were L\* direct (C1), the Gray profile from Lab PCS
//! (C2) and through sRGB as before (C3), the first meeting 1 code per cell at
//! 8 and 16 bits to ship. VALID: C1 ships, max |r| 0.267 code at 8 bits and
//! 0.011 at 16 (every candidate met: on an in-gamut scene the three agree,
//! so the declared order decides).
//!
//! # The blocker
//!
//! [`first_blocker`] is RGB → Gray's (`gray_convert::first_gray_blocker_with`)
//! with a Blend If channel named as a Lab document names it (Red, Green and
//! Blue are Lightness, a and b) and every adjustment layer refused with
//! `flatten_required`: a Lab record's settings act on L\*a\*b\* codes, not
//! Gray codes (Lab → CMYK's rule, `lab_cmyk`).
//!
//! # Memory
//!
//! The plan is RGB → Gray's (`gray_convert`'s `to_gray_scratch` over the same
//! pixels) plus [`extra_scratch_bytes`]. Held by
//! `scripts/lab-admission/measure.py` (`to-gray-lab16`); a miss raises
//! [`EXTRA_BYTES_PER_PIXEL`].

use crate::gray_convert::{first_gray_blocker_with, gray_rows_u16};
use crate::gray_document::GrayPolicy;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocumentLayer;
use crate::precision_image::PrecisionImage;

/// Bytes a converted pixel the Lab source charges over RGB → Gray's own
/// term (`gray_convert::TO_GRAY_BYTES_PER_PIXEL`). 0: the Lab arm reads each
/// u16 row and writes the same Gray plane through `gray_rows_u16`, with no
/// buffer beyond RGB → Gray's (no stage, no prepared pipeline). A
/// `to-gray-lab16` miss raises this term and no other.
pub(crate) const EXTRA_BYTES_PER_PIXEL: u64 = 0;

/// The Lab source's charge over `pixels` raster pixels (module doc,
/// "Memory").
pub(crate) fn extra_scratch_bytes(pixels: u64) -> u64 {
    pixels.saturating_mul(EXTRA_BYTES_PER_PIXEL)
}

/// The first record (postorder) Lab → Gray cannot carry, as a sentence
/// (module doc, "The blocker").
pub(crate) fn first_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    first_gray_blocker_with(layers, &crate::lab_style::channel_name, true)
}

/// One Lab layer image (its samples twin codes, L\* = 100 · c₀ / 65535) as a
/// Gray layer image in `policy`'s twin (module doc, "The rule").
pub(crate) fn gray_image(
    image: &PrecisionImage,
    policy: &GrayPolicy,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let code = |pixel: [u16; 4]| {
        let l_star = 100.0 * f64::from(pixel[0]) / 65535.0;
        policy.code(policy.encode(crate::icc_lut::lab_to_xyz([l_star, 0.0, 0.0])[1]))
    };
    gray_rows_u16(image, policy, &code, job)
}
