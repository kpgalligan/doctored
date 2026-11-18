//! The BAKE of a PSD vector mask Doctored does not carry (`psd_paths`): a
//! raster record's `vmsk` the record walk does not hold as a live vector
//! mask is drawn as ONE coverage of the shape with the file's per-subpath
//! operations over the canvas ([`vmsk_coverage`]) into the layer's raster
//! mask, at every depth. Since the walk carries a mask that holds a mix of
//! operations (`psd_paths::vmsk_holds`), what is baked is a mask that starts
//! from all pixels (flag bit 0 or an initial fill record of 1), one holding
//! a subpath of a single anchor beside its operations, and one the path
//! model does not admit or past the document's anchor cap; one naming an
//! operation word Doctored does not know is not drawn ([`bakes`]).
//!
//! The draw is `PathShape::coverage` of one shape holding `ops`, HS-2's
//! routine (`artwork_ops_coverage::rasterize_ops`), which folds the
//! subpaths' regions per run between crossings, so two subpaths sharing an
//! anti-aliased edge leave no seam. The mask starts from all pixels for an
//! initial fill record of 1 or a FIRST subpath that subtracts, while the
//! routine starts from everything exactly when the shape's first subpath
//! subtracts, so [`folded`] rewrites the head of the list by identities of
//! regions until the two agree. A subpath of one anchor is skipped, as the
//! bake has always skipped it, though it counts toward the caps and, as the
//! first, starts a subtracting mask from all pixels. Flag bit 0 inverts the
//! coverage last.
//!
//! The import draws only a mask within one shape's subpath and anchor caps
//! ([`bakes`]); one past them is dropped with the dropped-mask sentence. A
//! drawn mask is planned as a canvas plane (`psd_raster_masks::MaskBudget`)
//! and drawn under the open's job. The whole mask shares the rasterizer's
//! one work budget (`artwork_coverage::MAX_SCAN_WORK`) and its edge cap, so
//! a mask the rasterizer refuses refuses the layered open by the layer's
//! name ([`undrawn`]), never leaving the layer unmasked. The canvas's size
//! alone never makes it refuse (the assert below); only the outline's row
//! work or its edges can.

use crate::path_geometry::{FillRule, PathShape, Subpath, SubpathOp};
use crate::vector_mask_combine::op_from_word;

// Why the caps need no window rule: the bake's one window is the canvas,
// which the container bounds at `doc::MAX_PIXELS` before any record is
// walked (`psd_container`). The rasterizer refuses a window over that same
// `MAX_PIXELS` (`precision_image::pixel_count`) and charges its pixels up
// front against `artwork_coverage::MAX_SCAN_WORK`, so while this holds the
// window is never refused for its size, and the fold keeps at least
// `MAX_SCAN_WORK - MAX_PIXELS` for row work. That one charge is the whole
// mask's (`artwork_ops_coverage`: the canvas's pixels, then per row
// interval its runs times the subpaths present there); no per-subpath
// budget remains. Row work is the outline's, known only by running the
// sweep, so the report cannot predict it, and a mask past it refuses the
// open by the layer's name ([`undrawn`]). Raising `MAX_PIXELS` past
// `MAX_SCAN_WORK` stops the build here: the bake would then need to drop a
// mask on a canvas past the budget.
const _: () = assert!(crate::doc::MAX_PIXELS <= crate::artwork_coverage::MAX_SCAN_WORK);

/// A `vmsk` body as the bake draws it: `shape` holds its subpaths of two or
/// more anchors with their operations (one per subpath, nonzero), its head
/// rewritten ([`folded`]), or is empty, which is the constant `full`;
/// whether the mask starts from all pixels; whether its flags invert it
/// last.
struct Bake {
    shape: PathShape,
    full: bool,
    invert: bool,
}

impl Bake {
    /// `None` for a body that does not decode, names an unknown operation
    /// or is disabled (flag bit 2: it masks nothing). The counts are the
    /// file's subpaths and anchors, a skipped subpath among them.
    fn read(body: &[u8], canvas: (u32, u32)) -> Option<(Self, usize, usize)> {
        let (decoded, flags) = crate::psd_path_records::decode_vmsk(body, canvas).ok()?;
        if flags & 4 != 0 {
            return None;
        }
        // The file's words, never `decoded.shape.ops`, which a file whose
        // every word is 0 or every word is 1 does not hold.
        let operations: Vec<SubpathOp> = decoded
            .operations
            .iter()
            .map(|&word| op_from_word(word))
            .collect::<Option<_>>()?;
        let counts = (decoded.shape.subpaths.len(), decoded.shape.anchor_count());
        // From all pixels for an initial fill of 1, or a first subpath (of
        // any anchor count) that subtracts (`vector_mask_combine`).
        let full = decoded.inverted || operations.first() == Some(&SubpathOp::Subtract);
        let kept = decoded
            .shape
            .subpaths
            .into_iter()
            .zip(operations)
            .filter(|(subpath, _)| subpath.anchors.len() >= 2)
            .collect();
        let bake = Self {
            shape: folded(full, kept),
            full,
            invert: flags & 1 != 0,
        };
        Some((bake, counts.0, counts.1))
    }

    /// Its coverage over the whole `canvas` (module doc). The rows and
    /// columns the outline misses are the routine's own fold with every
    /// subpath outside (`artwork_ops_coverage::holds_outside`), so no second
    /// fill is written.
    fn draw(
        self,
        canvas: (u32, u32),
        job: crate::job_control::JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let mut coverage = if self.shape.subpaths.is_empty() {
            let n = crate::precision_image::pixel_count(canvas.0, canvas.1)?;
            let mut plane = crate::precision_image::reserved_vec(n)?;
            plane.resize(n, if self.full { u16::MAX } else { 0 });
            plane
        } else {
            self.shape.coverage((0, 0), canvas.0, canvas.1, true, job)?
        };
        if self.invert {
            for c in &mut coverage {
                *c = u16::MAX - *c;
            }
        }
        Ok(coverage)
    }
}

/// The fold from `full` (all pixels, or none) over `subpaths` in order, as
/// one shape whose own start (`artwork_ops_coverage`: everything exactly
/// when its first subpath subtracts) gives the same region. Only the head is
/// rewritten, by identities of regions. From all pixels a leading unite is
/// dropped (all ∪ S is all) and the rule repeats on the next; a subtract
/// stays; an intersect becomes a unite (all ∩ S is S); an exclude becomes a
/// subtract (all ⊕ S is all \ S). From none a leading subtract or intersect
/// is dropped (none \ S and none ∩ S are none) and the rule repeats; a unite
/// or an exclude stays (none ∪ S and none ⊕ S are S, and the routine starts
/// from none). Every subpath after the first kept is unchanged. When none is
/// kept the shape is empty and the mask the constant `full`.
fn folded(full: bool, subpaths: Vec<(Subpath, SubpathOp)>) -> PathShape {
    let mut rest = subpaths.into_iter().skip_while(|(_, op)| {
        if full {
            *op == SubpathOp::Unite
        } else {
            matches!(op, SubpathOp::Subtract | SubpathOp::Intersect)
        }
    });
    let Some((first, op)) = rest.next() else {
        return PathShape::default();
    };
    let head = match (full, op) {
        (true, SubpathOp::Intersect) => SubpathOp::Unite,
        (true, SubpathOp::Exclude) => SubpathOp::Subtract,
        (_, op) => op,
    };
    let (subpaths, ops): (Vec<Subpath>, Vec<SubpathOp>) =
        std::iter::once((first, head)).chain(rest).unzip();
    PathShape {
        subpaths,
        fill_rule: FillRule::NonZero,
        ops: Some(ops),
    }
}

/// Whether a `vmsk` body bakes ([`vmsk_coverage`]): it decodes, is enabled
/// (flag bit 2 clear), names only known operations and is within one
/// shape's caps (`path_geometry::MAX_SUBPATHS`, `MAX_PATH_ANCHORS`), counted
/// over the file's subpaths, a skipped one among them. Nothing else is
/// checked: the one window is the canvas (the assert above), and the row
/// work the rasterizer may refuse is known only by drawing it.
pub(crate) fn bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    use crate::path_geometry::{MAX_PATH_ANCHORS, MAX_SUBPATHS};
    Bake::read(body, canvas).is_some_and(|(_, subpaths, anchors)| {
        subpaths <= MAX_SUBPATHS && anchors <= MAX_PATH_ANCHORS
    })
}

/// The refusal of a layered open whose vector mask on layer `name` the
/// rasterizer does not draw: the report said the mask is drawn, so the
/// layer does not open unmasked instead. A carried vector mask's refusal
/// too (`psd_paths`).
pub(crate) fn undrawn(name: &str, reason: &str) -> String {
    format!("PSD vector mask on layer “{name}” could not be drawn: {reason}")
}

/// [`bakes`]: the seam a black-box test reads the drawing caps through.
#[doc(hidden)]
pub fn vmsk_bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    bakes(body, canvas)
}

/// [`vmsk_coverage`] with no job: the seam a black-box test reads the drawn
/// u16 samples through (an 8-bit open rounds them to bytes).
#[doc(hidden)]
pub fn vmsk_bake(body: &[u8], canvas: (u32, u32)) -> Option<Result<Vec<u16>, String>> {
    vmsk_coverage(body, canvas, crate::job_control::JobContext::NONE)
}

/// The coverage over the whole `canvas` of a `vmsk` body Doctored does not
/// hold as a live vector mask: one coverage of the shape with its
/// operations, from the start its initial fill and first subpath name,
/// inverted last when its flags say so (module doc), for an import that
/// keeps the picture as a raster mask. `None` for a body that does not
/// decode, names an unknown operation or is disabled (flag bit 2: it masks
/// nothing); `Some(Err)` is the rasterizer's refusal (its work budget and
/// edge cap, which the whole mask shares) or a cancellation.
pub(crate) fn vmsk_coverage(
    body: &[u8],
    canvas: (u32, u32),
    job: crate::job_control::JobContext<'_>,
) -> Option<Result<Vec<u16>, String>> {
    let (bake, _, _) = Bake::read(body, canvas)?;
    Some(bake.draw(canvas, job))
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
        layer.mask = Some(crate::doc::GrayRaster::new(plane));
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
        // The rasterizer's working memory over the canvas, the one window,
        // with the u16 plane it answers (`coverage_scratch`), then the mask
        // the layer keeps while `PrecisionPlane::adopt` copies that plane:
        // one more u16 plane.
        let scratch = crate::path_geometry::coverage_scratch(w, h)
            .saturating_add(2 * u64::from(w) * u64::from(h));
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
