//! Image ▸ Trim on a CMYK document (#547 phase 4b): the kept rectangle of
//! the COMPOSITE ink, through the one scan in `trim_bounds` (its master).
//! The composite is `cmyk_merge::cmyk_projection_image` over the canvas (the ONE
//! row compositor, straight CMYKA16), read with `CmykImage::sample_at`; a
//! pixel is transparent when its alpha (`[4]`) is 0, and the corner bases
//! compare all five codes exactly. Plan: `cmyk_merge_scratch_bytes` of the
//! canvas frame (Stamp Visible's, measured as `cmyk-admission`'s
//! `stamp-16`), admitted against the caller's limit beside `other`.

use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::trim_bounds::trim_rect;

impl PrecisionDocument {
    /// The kept rectangle `(x, y, w, h)` of the composite ink, or `None`
    /// when every pixel is trimmable or the basis is unknown (module doc).
    pub(crate) fn cmyk_trim_rect(
        &self,
        basis: u32,
        edges: u32,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<(u32, u32, u32, u32)>, String> {
        self.require_cmyk_policy("Trim")?;
        let frame = ((0, 0), self.dimensions());
        admit(
            self.retained_bytes(),
            self.cmyk_merge_scratch_bytes(frame)?,
            other,
            limit,
        )?;
        let image = self.cmyk_projection_image(frame, job)?;
        let (w, h) = image.dimensions();
        let at = |i: usize| {
            image
                .sample_at((i % w as usize) as u32, (i / w as usize) as u32)
                .unwrap_or_default()
        };
        Ok(trim_rect(w, h, |i| at(i)[4] == 0, at, basis, edges))
    }
}
