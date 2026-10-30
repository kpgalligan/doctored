//! Type, shape and fill layers, Rasterize Artwork, paths and vector masks on
//! a Lab Color document (#547 Lab phase 6a-i, `designs/lab-mode.md`):
//! the rules, and the 8-bit publication grid ([`on_document_grid`]).
//!
//! # Why the existing renderer, unchanged
//!
//! A Lab document's storage is the L\*a\*b\* codes in the Lab twin, u16
//! (`lab_document`'s module doc). An artwork description renders through the
//! one renderer (`artwork_render`, the host's text layout) into the twin, and
//! the twin has no matrix model, so every authored colour — coverage, fill,
//! gradient stop, `linear` wide colour, intrinsic glyph pixel — enters the
//! document by `authored_color::lut_ink` on the twin: relative colorimetric,
//! no BPC, Image ▸ Mode ▸ Lab Color's rule. A pattern fill's tile converts
//! texel by texel before the pyramid (`lab_style::twin_tile_sampler`, the Lab
//! Pattern Overlay's rule). There is no second renderer.
//!
//! # The rules (Doctored's, no Photoshop take)
//!
//! * **Descriptions stay authored sRGB.** A run's, a shape's or a fill
//!   layer's colours are the sRGB swatch the host authored (an Eyedropper
//!   L\*a\*b\* outside sRGB authors its clipped swatch), so the stored
//!   description, `get_document`, the `.rz` body and the render agree, and
//!   a Mode conversion carries the description unchanged.
//! * **A gradient** interpolates between its converted stops' codes.
//! * **At 8 bits** the published image is put on the ×257 grid
//!   ([`on_document_grid`], `lab_depth::on_grid`, the 16 → 8 depth edit's
//!   rule), so an 8-bit Lab type, shape or fill layer holds 8-bit codes from
//!   its first publication.
//! * **Mode conversions** (Lab → RGB, CMYK, Gray and back, `lab_convert::
//!   first_lab_blocker` no longer refuses) convert a described layer as its
//!   pixels with the description dropped in the core, as `cmyk_convert_tree`
//!   and `gray_convert` do; the host re-renders the layer from its
//!   description in the destination. 8/16 Bits/Channel keeps the
//!   description (`lab_depth`).
//! * **Paths and vector masks** are geometry with no colour: they ride the
//!   document as on RGB.
//! * **Export as PSD** writes a Lab fill layer as its rendered codes
//!   (`psd_export16::exported_fill16` answers `None` on the twin), reported
//!   by preflight's described-layer loss; Lab `SoCo`/`GdFl`/`PtFl` and `TySh`
//!   records are unmeasured.
//!
//! # Memory
//!
//! [`grid_bytes`] is the one image [`on_document_grid`] may build, 8 B/px of
//! u16 RGBA, added to the publication's scratch beside Gray's fold.

use crate::job_control::JobContext;
use crate::lab_document::{LabDepth, LabPolicy};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionRun};

/// `image`, a publication on a Lab document of `policy`, on the ×257 grid
/// (module doc): `None` at 16 bits or when every sample and alpha already is
/// on the grid (the caller keeps its own image, so an unchanged layer still
/// compares equal), else `lab_depth::on_grid`'s image.
pub(crate) fn on_document_grid(
    image: &PrecisionImage,
    policy: &LabPolicy,
    job: JobContext<'_>,
) -> Result<Option<PrecisionImage>, String> {
    if policy.depth() != LabDepth::Eight {
        return Ok(None);
    }
    let (w, h) = image.dimensions();
    let mut on = true;
    'rows: for y in 0..h {
        for (_, run) in image.row_segments(y, 0, w) {
            let gridded = match run {
                PrecisionRun::Rgba16(run) => run.iter().all(|p| p.iter().all(|v| v % 257 == 0)),
                // A Lab document is never float; `on_grid` refuses it.
                PrecisionRun::Rgba32(_) => false,
            };
            if !gridded {
                on = false;
                break 'rows;
            }
        }
        // Cancellation only: `on_grid` below reports this job's progress.
        job.check()?;
    }
    if on {
        return Ok(None);
    }
    crate::lab_depth::on_grid(image, job).map(Some)
}

/// The bytes [`on_document_grid`] may allocate for an image of `pixels`
/// pixels on `doc`: 8 B/px of u16 RGBA on an 8-bit Lab document, else 0.
pub(crate) fn grid_bytes(doc: &PrecisionDocument, pixels: u64) -> u64 {
    match doc.lab_policy() {
        Some(policy) if policy.depth() == LabDepth::Eight => pixels.saturating_mul(8),
        _ => 0,
    }
}
