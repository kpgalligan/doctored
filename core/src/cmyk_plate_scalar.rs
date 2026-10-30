//! ONE INK PLATE AS A SCALAR TARGET on a CMYK document (#547): painting,
//! filling, a gradient, the Paint Bucket, Dodge/Burn, morphology, a filter or
//! an adjustment, and `replace`/`replace_canvas` (Apply Image, Calculations)
//! on the Cyan, Magenta, Yellow or Black row the Channels panel targets,
//! wherever an RGB document admits the same on one colour channel. An
//! adapter for `precision_scalar_edit` (its `ScalarTarget` plate
//! arms), `precision_plane_read` and so `precision_scalar_statistics`: the
//! scalar kernels run unchanged on the plate's numbers.
//!
//! * THE DOMAIN is the stored `1 − ink`: sample `65535 − ink[plate]` over the
//!   layer rect. It is the number Load Selection reads from a plate
//!   (`cmyk_composite::plate_coverage`, `cmyk_select`'s `color` kind) and the
//!   Channels panel shows (`cmyk_render`'s plate views), so white leaves no
//!   ink and a grey g painted on a plate leaves `ink = 1 − g`.
//! * THE WRITE replaces plate k only: `ink[plate] = 65535 − s`; the other
//!   three plates and the alpha keep every stored sample, and tiles whose
//!   samples did not change stay shared (`CmykImage::with_rect`). A write that
//!   changes nothing is `Ok(None)`.
//! * LOCKS are the RGB plane's (`EditKind::Pixels`, applied by
//!   `ScalarEdit::apply`). A record other than a CMYK raster (a group, an
//!   adjustment) refuses with [`PLATE_REFUSAL`].
//!
//! PLAN. `ScalarEdit::plan`'s terms plus, for a plate, the CMYKA16 read and
//! the rebuilt rect (n × 2 × `CMYK_BYTES_PER_PIXEL`) and the document's display
//! preparation of the result (canvas × `cmyk_adjust_layer::
//! DISPLAY_BYTES_PER_PIXEL`). `scripts/cmyk-admission`'s `plate-fill-16` and
//! `plate-gaussian-16` cases are its measured record.

use crate::cmyk_image::CmykImage;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::reserved_vec;

/// A plate's stored dimensions and its canvas offset (the layer rect).
type PlateGeometry = ((u32, u32), (i32, i32));

/// The refusal of a plate target on anything but a CMYK raster record.
pub(crate) const PLATE_REFUSAL: &str = "a CMYK ink plate requires a CMYK raster layer";

impl PrecisionDocument {
    /// Record `index`'s CMYK image and offset (module doc's refusal).
    fn cmyk_plate_image(&self, index: usize) -> Result<(&CmykImage, (i32, i32)), String> {
        match self.layer(index).map(|record| &record.content) {
            Some(Content::CmykRaster { image, offset }) => Ok((image, *offset)),
            _ => Err(PLATE_REFUSAL.into()),
        }
    }

    /// The plate's stored dimensions and canvas offset (the layer rect).
    pub(crate) fn cmyk_plate_geometry(&self, index: usize) -> Result<PlateGeometry, String> {
        let (image, offset) = self.cmyk_plate_image(index)?;
        Ok((image.dimensions(), offset))
    }

    /// Plate `plate`'s numbers over the layer rect, row-major: `65535 −
    /// ink[plate]` (module doc's domain).
    pub(crate) fn cmyk_plate_samples(
        &self,
        index: usize,
        plate: usize,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let (image, _) = self.cmyk_plate_image(index)?;
        let (w, h) = image.dimensions();
        let stored = image.read_rect((0, 0, w, h), job)?;
        let mut out = reserved_vec(stored.len())?;
        for (i, sample) in stored.iter().enumerate() {
            job.every(i)?;
            out.push(65535 - sample[plate]);
        }
        Ok(out)
    }

    /// The document with plate `plate` of record `index` set from `samples`
    /// (`ink = 65535 − s`, module doc's write); `Ok(None)` when no stored
    /// sample changed.
    pub(crate) fn replacing_cmyk_plate(
        &self,
        index: usize,
        plate: usize,
        samples: &[u16],
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        self.require_cmyk_policy("a CMYK ink plate")?;
        let (image, offset) = self.cmyk_plate_image(index)?;
        let (w, h) = image.dimensions();
        let mut stored = image.read_rect((0, 0, w, h), job)?;
        if stored.len() != samples.len() {
            return Err("a CMYK ink plate write must cover the layer rect".into());
        }
        for (i, (dst, s)) in stored.iter_mut().zip(samples).enumerate() {
            job.every(i)?;
            dst[plate] = 65535 - s;
        }
        let Some(image) = image.with_rect((0, 0, w, h), &stored, job)? else {
            return Ok(None);
        };
        self.replacing_content(index, Content::CmykRaster { image, offset })
    }
}
