//! The two `L` passes a CMYK edit runs through a shipped RGB16 operation
//! (#1): Filter ▸ … (`cmyk_filter`) and the retouch tools
//! (`cmyk_retouch_brush`, `cmyk_retouch`).
//!
//! The stored domain is `L = 65535 − ink`. [`InkPass::Cmy`] is the RGBA16
//! sample `[L_c, L_m, L_y, α]`, [`InkPass::K`] is `[L_k, L_k, L_k, α]`. A
//! PASS DOCUMENT is a one-layer RGBA16 `PrecisionDocument` tagged with the
//! built-in sRGB (the numbers are `L`, not a colour) whose layer 0 holds one
//! pass of a CMYK image at that image's offset on a canvas of the CMYK
//! document's size, so the RGB operation reads the canvas coverage, the
//! selection, the dabs and the source offsets exactly as it reads them on an
//! RGB document. `blank` gives the image's extent as paper at α 0,
//! `[65535, 65535, 65535, 0]`, what the Clone Stamp paints its source onto. A
//! pass document of a transparency-locked record carries the lock, so the
//! RGB walk takes its own frozen-alpha arithmetic.
//!
//! [`join_sample`] maps the two results back: C/M/Y = 65535 − A's R/G/B,
//! K = 65535 − B's G (or the source's K when the edit runs pass A only, the
//! K plate held exactly), α = A's — or the source's under a transparency
//! lock. A pixel whose final α is 0 keeps its source ink. The two passes'
//! alphas must agree (every pass edit moves α by the same coverage
//! arithmetic on both); a disagreement is an internal error, never a guess.

use crate::cmyk_image::{CmykImage, CmykSample};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage};

/// The paper sample of a blank pass: no ink, no coverage.
pub(crate) const BLANK_PASS: [u16; 4] = [65535, 65535, 65535, 0];

/// One of the two `L` passes (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InkPass {
    Cmy,
    K,
}

impl InkPass {
    /// `s` as this pass's RGBA16 sample.
    pub(crate) fn sample(self, s: CmykSample) -> [u16; 4] {
        match self {
            Self::Cmy => [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]],
            Self::K => [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]],
        }
    }
}

/// The module doc's join of one pixel: `a` pass A's result, `b` pass B's
/// (`None`: the K plate held), `frozen` a transparency lock.
pub(crate) fn join_sample(
    source: CmykSample,
    a: [u16; 4],
    b: Option<[u16; 4]>,
    frozen: bool,
) -> CmykSample {
    let alpha = if frozen { source[4] } else { a[3] };
    if alpha == 0 {
        [source[0], source[1], source[2], source[3], 0]
    } else {
        [
            65535 - a[0],
            65535 - a[1],
            65535 - a[2],
            b.map_or(source[3], |b| 65535 - b[1]),
            alpha,
        ]
    }
}

/// `image`'s pass `pass` as an RGBA16 image of its own size (`blank`: paper
/// at α 0).
pub(crate) fn pass_image(
    image: &CmykImage,
    pass: InkPass,
    blank: bool,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    if blank {
        return PrecisionImage::filled_rgba16(w, h, BLANK_PASS, IccProfile::srgb());
    }
    let mut pixels = reserved_vec(pixel_count(w, h)?)?;
    for y in 0..h {
        job.check()?;
        for (_, run) in image.row_segments(y, 0, w) {
            pixels.extend(run.iter().map(|s| pass.sample(*s)));
        }
    }
    PrecisionImage::from_rgba16_unseeded(w, h, pixels, IccProfile::srgb(), None)
}

/// A pass document (module doc) of a `canvas`-sized document whose layer 0
/// is `image` at `offset`, `locks` its lock bits.
pub(crate) fn pass_document(
    canvas: (u32, u32),
    image: PrecisionImage,
    offset: (i32, i32),
    locks: u32,
) -> Result<PrecisionDocument, String> {
    let layer = PrecisionDocumentLayer::raster("pass", image, offset, 1.0, true)?.with_locks(locks);
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
}

/// Layer 0's image of a pass document, a pass result's.
pub(crate) fn pass_result(doc: &PrecisionDocument) -> Result<&PrecisionImage, String> {
    doc.layer(0)
        .and_then(PrecisionDocumentLayer::source)
        .ok_or_else(|| "internal error: a CMYK pass lost its layer".to_string())
}

/// `dest` joined with pass results `a` and `b` of its own size (module doc);
/// `None` when no stored code changes.
pub(crate) fn join_images(
    dest: &CmykImage,
    a: &PrecisionImage,
    b: Option<&PrecisionImage>,
    frozen: bool,
    job: JobContext<'_>,
) -> Result<Option<CmykImage>, String> {
    let (w, h) = dest.dimensions();
    if a.dimensions() != (w, h) || b.is_some_and(|b| b.dimensions() != (w, h)) {
        return Err("internal error: a CMYK pass changed the layer's size".into());
    }
    let mut changed = false;
    let out = CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0u32;
            for (_, run) in dest.row_segments(y, 0, w) {
                for s in run {
                    let pa = a.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                    let pb = match b {
                        Some(b) => {
                            let pb = b.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                            if pb[3] != pa[3] {
                                return Err(
                                    "internal error: the CMYK passes disagree on alpha".into()
                                );
                            }
                            Some(pb)
                        }
                        None => None,
                    };
                    let next = join_sample(*s, pa, pb, frozen);
                    changed |= next != *s;
                    row[x as usize] = next;
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    Ok(changed.then_some(out))
}

impl PrecisionDocument {
    /// CMYK raster `layer`'s pass document (module doc): canvas-sized, the
    /// pass at the record's offset, carrying a transparency lock the record
    /// is under (never for a `blank` pass, which has no alpha to keep).
    pub(crate) fn cmyk_pass_document(
        &self,
        layer: usize,
        pass: InkPass,
        blank: bool,
    ) -> Result<PrecisionDocument, String> {
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record
            .cmyk_source()
            .ok_or("a CMYK edit requires a CMYK raster layer")?;
        let locks = if !blank && self.lock_owner(layer, LOCK_TRANSPARENCY).is_some() {
            LOCK_TRANSPARENCY
        } else {
            0
        };
        pass_document(
            self.dimensions(),
            pass_image(image, pass, blank, JobContext::NONE)?,
            record.offset(),
            locks,
        )
    }

    /// A canvas-sized sampling plate's pass document (module doc): `plate`
    /// at the origin.
    pub(crate) fn cmyk_plate_document(
        plate: &CmykImage,
        pass: InkPass,
    ) -> Result<PrecisionDocument, String> {
        pass_document(
            plate.dimensions(),
            pass_image(plate, pass, false, JobContext::NONE)?,
            (0, 0),
            0,
        )
    }

    /// CMYK raster `layer` joined with pass documents `a` and `b` (module
    /// doc; `b` `None` holds K), as the document with the record's ink
    /// replaced; `None` when no stored code changes.
    pub(crate) fn cmyk_joined(
        &self,
        layer: usize,
        a: &PrecisionDocument,
        b: Option<&PrecisionDocument>,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record
            .cmyk_source()
            .ok_or("a CMYK edit requires a CMYK raster layer")?;
        let offset = record.offset();
        let frozen = self.lock_owner(layer, LOCK_TRANSPARENCY).is_some();
        let b = b.map(pass_result).transpose()?;
        match join_images(image, pass_result(a)?, b, frozen, job)? {
            Some(image) => self.replacing_content(layer, Content::CmykRaster { image, offset }),
            None => Ok(None),
        }
    }
}
