//! The BAKE of a PSD vector mask Doctored does not carry (`psd_paths`): a
//! raster record's `vmsk` the record walk does not hold as a live vector
//! mask is drawn under its per-subpath operations ([`vmsk_coverage`],
//! `vector_mask_combine`) into the layer's raster mask, at every depth. The
//! import draws only a mask within the drawing caps ([`bakes`]: one shape's
//! subpath and anchor caps, and [`MAX_BAKE_SAMPLES`] of canvas passes); one
//! past them is dropped with the dropped-mask sentence. A drawn mask is
//! planned as a canvas plane (`psd_raster_masks::MaskBudget`), drawn under
//! the open's job, and a mask the rasterizer refuses refuses the layered
//! open by the layer's name, never leaving the layer unmasked.

use crate::path_geometry::PathShape;

/// The most samples the import spends drawing one vector mask it does not
/// carry ([`vmsk_coverage`]): each subpath is a pass over the whole canvas
/// (a rasterization and a combine), so the work is subpaths × canvas
/// pixels. 2³⁰ is ten passes over a 100 MP canvas and over 250 over a 4 MP
/// one: far past any mask a person draws, and a bound on what one record of
/// a crafted file can cost the open.
pub(crate) const MAX_BAKE_SAMPLES: u64 = 1 << 30;

/// Whether a `vmsk` body bakes ([`vmsk_coverage`]): it decodes, is enabled
/// (flag bit 2 clear), names only known operations and is within the
/// drawing caps — one shape's own (`path_geometry::MAX_SUBPATHS`,
/// `MAX_PATH_ANCHORS`) and [`MAX_BAKE_SAMPLES`] of passes over `canvas`.
pub(crate) fn bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    use crate::path_geometry::{MAX_PATH_ANCHORS, MAX_SUBPATHS};
    crate::psd_path_records::decode_vmsk(body, canvas).is_ok_and(|(decoded, flags)| {
        let subpaths = decoded.shape.subpaths.len();
        let passes = (subpaths as u64)
            .saturating_mul(u64::from(canvas.0))
            .saturating_mul(u64::from(canvas.1));
        flags & 4 == 0
            && subpaths <= MAX_SUBPATHS
            && decoded.shape.anchor_count() <= MAX_PATH_ANCHORS
            && passes <= MAX_BAKE_SAMPLES
            && decoded
                .operations
                .iter()
                .all(|&word| crate::vector_mask_combine::SubpathOp::from_word(word).is_some())
    })
}

/// The refusal of a layered open whose vector mask on layer `name` the
/// rasterizer does not draw: the report said the mask is drawn, so the
/// layer does not open unmasked instead.
fn undrawn(name: &str, reason: &str) -> String {
    format!("PSD vector mask on layer “{name}” could not be drawn: {reason}")
}

/// [`bakes`]: the seam a black-box test reads the drawing caps through.
#[doc(hidden)]
pub fn vmsk_bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    bakes(body, canvas)
}

/// The coverage over the whole `canvas` of a `vmsk` body Doctored does not
/// hold as a live vector mask, under its per-subpath operations, initial
/// fill and inverted flag (`vector_mask_combine`), for an import that keeps
/// the picture as a raster mask. `None` for a body that does not decode,
/// names an unknown operation or is disabled (flag bit 2: it masks
/// nothing); `Some(Err)` is the rasterizer's refusal or a cancellation.
pub(crate) fn vmsk_coverage(
    body: &[u8],
    canvas: (u32, u32),
    job: crate::job_control::JobContext<'_>,
) -> Option<Result<Vec<u16>, String>> {
    use crate::vector_mask_combine::{apply, finish, start, SubpathOp};
    let (decoded, flags) = crate::psd_path_records::decode_vmsk(body, canvas).ok()?;
    if flags & 4 != 0 {
        return None;
    }
    let operations: Vec<SubpathOp> = decoded
        .operations
        .iter()
        .map(|&word| SubpathOp::from_word(word))
        .collect::<Option<_>>()?;
    let (w, h) = canvas;
    // A first subpath that subtracts starts from all pixels (`vector_mask_combine`).
    let full = decoded.inverted || operations.first() == Some(&SubpathOp::Subtract);
    let mut acc = start(w as usize * h as usize, full);
    for (subpath, op) in decoded.shape.subpaths.into_iter().zip(operations) {
        let one = PathShape {
            subpaths: vec![subpath],
            fill_rule: crate::path_geometry::FillRule::NonZero,
        };
        if one.subpaths[0].anchors.len() < 2 {
            continue;
        }
        match one.coverage((0, 0), w, h, true, job) {
            Ok(coverage) => apply(&mut acc, op, &coverage),
            Err(reason) => return Some(Err(reason)),
        }
    }
    finish(&mut acc, flags & 1 != 0);
    Some(Ok(acc))
}

impl crate::doc::DrDocument {
    /// Entry `idx` with the raster mask a `vmsk` Doctored cannot hold as a
    /// vector mask draws ([`vmsk_coverage`]) over the canvas, independent
    /// (unlinked) and enabled; the document unchanged when the body does not
    /// bake. Installed past the layer's locks, as the vector masks are.
    /// `Err` is a cancellation, or the rasterizer's refusal by the layer's
    /// name (the open is refused, never left unmasked).
    pub(crate) fn importing_baked_vector_mask(
        mut self,
        idx: usize,
        body: &[u8],
        job: crate::job_control::JobContext<'_>,
    ) -> Result<Self, String> {
        let (w, h) = (self.width, self.height);
        let coverage = match vmsk_coverage(body, (w, h), job) {
            None => return Ok(self),
            Some(Ok(coverage)) => coverage,
            Some(Err(reason)) => {
                job.check()?;
                let name = self.layers.get(idx).map_or("", |layer| layer.name.as_str());
                return Err(undrawn(name, &reason));
            }
        };
        // A u16 sample as a byte, rounded to nearest (`doc_vector_mask`'s).
        let bytes: Vec<u8> = coverage
            .iter()
            .map(|&c| ((u32::from(c) * 255 + 32767) / 65535) as u8)
            .collect();
        let (Some(plane), Some(layer)) = (
            image::GrayImage::from_raw(w, h, bytes),
            self.layers.get_mut(idx),
        ) else {
            return Ok(self);
        };
        let mut properties = crate::doc_mask::MaskProperties::at((0, 0));
        properties.linked = false;
        layer.mask = Some(std::sync::Arc::new(plane));
        layer.mask_enabled = true;
        layer.mask_properties = properties;
        Ok(self)
    }
}

impl crate::precision_document::PrecisionDocument {
    /// The typed twin of `DrDocument::importing_baked_vector_mask`: a
    /// canvas-sized u16 mask, its planes admitted against `limit` / `other`
    /// (a refused admission refuses the open, as every other import charge
    /// does). `Err` is also a cancellation, or the rasterizer's refusal by
    /// the layer's name.
    pub(crate) fn importing_baked_vector_mask(
        self,
        index: usize,
        body: &[u8],
        limit: u64,
        other: u64,
        job: crate::job_control::JobContext<'_>,
    ) -> Result<Self, String> {
        let (w, h) = self.dimensions();
        // One subpath's coverage (the rasterizer's charge), then the
        // accumulator and the mask the layer keeps, two u16 planes.
        let scratch = crate::path_geometry::coverage_scratch(w, h)
            .saturating_add(4 * u64::from(w) * u64::from(h));
        crate::precision_image::admit(self.retained_bytes(), scratch, other, limit)?;
        let coverage = match vmsk_coverage(body, (w, h), job) {
            None => return Ok(self),
            Some(Ok(coverage)) => coverage,
            Some(Err(reason)) => {
                job.check()?;
                let name = self.layer(index).map_or("", |layer| layer.name());
                return Err(undrawn(name, &reason));
            }
        };
        let Some(mut layer) = self.layer(index).cloned() else {
            return Ok(self);
        };
        let mut properties = crate::doc_mask::MaskProperties::at((0, 0));
        properties.linked = false;
        layer.mask = Some(crate::precision_mask::PrecisionMask::new(
            crate::precision_mask::PrecisionPlane::adopt(w, h, coverage)?,
            properties,
            true,
        )?);
        Ok(self.replacing_layer(index, layer)?.unwrap_or(self))
    }
}
