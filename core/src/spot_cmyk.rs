//! The spot channels on a CMYK document's composite (#1 1c-1,
//! `designs/spot-channels.md`): the adapter between a CMYK
//! document's spot channels and the kernel's CMYK arm
//! (`spot_overprint::overprint_cmyka16`).
//!
//! A spot ink on a CMYK document overprints as ink on paper: the press
//! separations carry it. So the overprint runs on the composite ON
//! PAPER, in ink, never through the display: multiplying through an RGB view
//! would hand the press an RGB-derived ink. The ink's own CMYK is its D50
//! Lab through the document's CMYK profile ([`spot_ink_cmyk`]: Lab → PCS
//! XYZ, `icc_lut::lab_to_xyz`, then `cmyk_color::ToDevice` from the Lab twin
//! at relative colorimetric with no BPC, each plate clamped to `[0, 1]`).
//!
//! [`PrecisionDocument::cmyk_spot_on_paper`] is `cmyk_on_paper` with each
//! listed spot channel overprinted in list order as each row is laid, so the
//! overprint costs no plane beyond the on-paper image. Its callers are the
//! converted export (rows from it instead of the stack), the kept export
//! (whose staged branch it fills) and the JPEG-intermediate save; the CMYK
//! TIFF save and every PSD write keep `cmyk_flat_samples`, whose spot planes
//! are written as channels (TIFF's path is 1c-2).
//!
//! # Memory
//!
//! [`PrecisionDocument::cmyk_spot_scratch_bytes`]: the on-paper image a
//! caller that otherwise rows from the stack now holds (canvas ×
//! `CMYK_BYTES_PER_PIXEL`) and bookkeeping: the converted export and the
//! JPEG-intermediate save add it. Merge Spot Channel and the kept export do
//! not: their plans already charge the on-paper plane the overprint fills in
//! place (`cmyk_flatten_scratch_bytes`, the kept staged plan), which
//! `scripts/spot-admission/measure.py` holds (`merge-cmyk16`,
//! `export-cmyk-jpeg`).

use crate::cmyk_color::ToDevice;
use crate::cmyk_image::{CmykImage, CMYK_BYTES_PER_PIXEL};
use crate::icc_intent::RenderingIntent;
use crate::job_control::JobContext;
use crate::lab_document::lab_twin;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::BOOKKEEPING_BYTES;
use crate::spot_overprint::overprint_cmyka16;

/// Spot channel `index`'s ink as C, M, Y and K in `[0, 1]` through `doc`'s
/// CMYK profile (module doc). `Err` for a document that is not CMYK, an
/// index out of range, an alpha channel, or a profile with no PCS-to-device
/// table (a read-only profile a CMYK document may hold): no ink is invented,
/// so a merge or an export refuses rather than printing nothing. Public only
/// as the black-box tests' seam (their CMYK oracle runs on this ink).
#[doc(hidden)]
pub fn spot_ink_cmyk(doc: &PrecisionDocument, index: usize) -> Result<[f32; 4], String> {
    let policy = doc.require_cmyk_policy("A spot ink in CMYK")?;
    let (_, ink) = doc.spot_plane(index)?;
    let intent = RenderingIntent::RelativeColorimetric;
    let leg = ToDevice::new(&lab_twin(), &policy.profile, intent, false);
    let xyz = crate::icc_lut::lab_to_xyz(ink.lab());
    let (device, _) = leg.device(&policy.profile, xyz).ok_or_else(|| {
        format!(
            "the spot ink cannot be converted into the CMYK profile \"{}\", which has no B2A (PCS-to-device) table",
            policy.name()
        )
    })?;
    Ok(std::array::from_fn(|i| device[i].clamp(0.0, 1.0) as f32))
}

impl PrecisionDocument {
    /// The composite on paper, as `cmyk_on_paper` gives it, with spot
    /// channels `channels` overprinted in ink in list order (module doc).
    /// `Err` for an index that is not a spot channel, a document that is
    /// not CMYK and a cancel. Admission belongs to the caller
    /// ([`Self::cmyk_spot_scratch_bytes`]).
    pub(crate) fn cmyk_spot_on_paper(
        &self,
        channels: &[usize],
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let mut passes = Vec::with_capacity(channels.len());
        for &index in channels {
            let (coverage, ink) = self.spot_plane(index)?;
            let solidity = f32::from(ink.solidity()) / 100.0;
            passes.push((coverage, spot_ink_cmyk(self, index)?, solidity));
        }
        let width = self.dimensions().0 as usize;
        self.cmyk_on_paper(job, |y, row| {
            let start = y as usize * width;
            for (coverage, ink, solidity) in &passes {
                let line = coverage
                    .get(start..start + row.len())
                    .ok_or("a spot channel does not match this document's canvas")?;
                if !overprint_cmyka16(row, line, *ink, *solidity) {
                    return Err("a spot channel does not match this document's canvas".into());
                }
            }
            Ok(())
        })
    }

    /// The on-paper image's charge (module doc, "Memory").
    pub(crate) fn cmyk_spot_scratch_bytes(&self) -> u64 {
        let (w, h) = self.dimensions();
        (u64::from(w) * u64::from(h))
            .saturating_mul(CMYK_BYTES_PER_PIXEL)
            .saturating_add(BOOKKEEPING_BYTES)
    }
}
