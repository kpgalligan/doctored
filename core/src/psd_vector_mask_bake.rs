//! The BAKE of a PSD vector mask Doctored does not carry (`psd_paths`): a
//! raster record's `vmsk` the record walk does not hold as a live vector
//! mask is drawn under its per-subpath operations ([`vmsk_coverage`],
//! `vector_mask_combine`) into the layer's raster mask, at every depth. Each
//! subpath is drawn over its own window — its control hull
//! (`PathShape::bounds`, which holds the curve) widened to whole pixels and
//! by one more on every side, clamped to the canvas ([`hull`]) — under
//! `vector_mask_combine::charge`'s rule, so a small mask on a large canvas
//! costs its own size, and the picture is the whole-canvas draw's. The
//! import draws only a mask within the drawing caps ([`bakes`]: one shape's
//! subpath and anchor caps, and [`MAX_BAKE_SAMPLES`] of those windows); one
//! past them is dropped with the dropped-mask sentence. A drawn mask is
//! planned as a canvas plane (`psd_raster_masks::MaskBudget`), drawn under
//! the open's job, and a mask the rasterizer refuses refuses the layered
//! open by the layer's name, never leaving the layer unmasked.

use crate::path_geometry::PathShape;
use crate::vector_mask_combine::{charge, Accumulator, SubpathOp, Window};

/// The most samples the import spends drawing one vector mask it does not
/// carry ([`vmsk_coverage`]): the sum over its subpaths of the window each
/// is drawn over (a rasterization and a combine there), an intersect
/// charged the whole box of what the mask held so far, which it combines
/// inside its window and clears outside it (`vector_mask_combine::charge`).
/// No subpath is charged more than the canvas. The fill a mask starts from
/// and its final inversion are one canvas pass each per mask, outside the
/// cap. 2³⁰ is ten whole-canvas subpaths on a 100 MP canvas and over 250 on
/// a 4 MP one: far past any mask a person draws, and a bound on what one
/// record of a crafted file can cost the open.
pub(crate) const MAX_BAKE_SAMPLES: u64 = 1 << 30;

/// A `vmsk` body as the bake draws it: each subpath of two or more anchors
/// as its own shape beside its operation (one of a single anchor is skipped
/// and charged nothing, as the bake has always skipped it, though it still
/// counts toward the subpath cap and, as the first, starts a subtracting
/// mask from all pixels), whether the mask starts from all pixels, and
/// whether its flags invert it last (`vector_mask_combine`).
struct Bake {
    subpaths: Vec<(PathShape, SubpathOp)>,
    full: bool,
    invert: bool,
}

impl Bake {
    /// `None` for a body that does not decode, names an unknown operation
    /// or is disabled (flag bit 2: it masks nothing).
    fn read(body: &[u8], canvas: (u32, u32)) -> Option<(Self, usize, usize)> {
        let (decoded, flags) = crate::psd_path_records::decode_vmsk(body, canvas).ok()?;
        if flags & 4 != 0 {
            return None;
        }
        let operations: Vec<SubpathOp> = decoded
            .operations
            .iter()
            .map(|&word| SubpathOp::from_word(word))
            .collect::<Option<_>>()?;
        let counts = (decoded.shape.subpaths.len(), decoded.shape.anchor_count());
        // A first subpath that subtracts starts from all pixels (`vector_mask_combine`).
        let full = decoded.inverted || operations.first() == Some(&SubpathOp::Subtract);
        let subpaths = decoded
            .shape
            .subpaths
            .into_iter()
            .zip(operations)
            .filter(|(subpath, _)| subpath.anchors.len() >= 2)
            .map(|(subpath, op)| {
                let one = PathShape {
                    subpaths: vec![subpath],
                    fill_rule: crate::path_geometry::FillRule::NonZero,
                };
                (one, op)
            })
            .collect();
        let bake = Self {
            subpaths,
            full,
            invert: flags & 1 != 0,
        };
        Some((bake, counts.0, counts.1))
    }

    /// The live box the mask starts from (`vector_mask_combine`).
    fn live(&self, canvas: (u32, u32)) -> Window {
        if self.full {
            Window::canvas(canvas.0, canvas.1)
        } else {
            Window::default()
        }
    }

    /// The samples drawing it charges ([`MAX_BAKE_SAMPLES`]), saturating.
    fn samples(&self, canvas: (u32, u32)) -> u64 {
        let mut live = self.live(canvas);
        self.subpaths.iter().fold(0u64, |sum, (one, op)| {
            sum.saturating_add(charge(&mut live, *op, hull(one, canvas)).1)
        })
    }
}

/// The window a subpath `one` is drawn over on `canvas` (module doc): its
/// control hull widened to whole pixels and by one pixel on every side,
/// clamped to the canvas; empty when that misses the canvas.
fn hull(one: &PathShape, canvas: (u32, u32)) -> Window {
    let Some([x0, y0, x1, y1]) = one.bounds() else {
        return Window::default();
    };
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    // Clamped as f64, so a coordinate far off the canvas never wraps a cast.
    let left = (x0.floor() - 1.0).clamp(0.0, w);
    let top = (y0.floor() - 1.0).clamp(0.0, h);
    let right = (x1.ceil() + 1.0).clamp(0.0, w);
    let bottom = (y1.ceil() + 1.0).clamp(0.0, h);
    if right <= left || bottom <= top {
        return Window::default();
    }
    Window {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    }
}

/// Whether a `vmsk` body bakes ([`vmsk_coverage`]): it decodes, is enabled
/// (flag bit 2 clear), names only known operations and is within the
/// drawing caps — one shape's own (`path_geometry::MAX_SUBPATHS`,
/// `MAX_PATH_ANCHORS`) and [`MAX_BAKE_SAMPLES`] of the windows its
/// subpaths are drawn over on `canvas`.
pub(crate) fn bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    use crate::path_geometry::{MAX_PATH_ANCHORS, MAX_SUBPATHS};
    Bake::read(body, canvas).is_some_and(|(bake, subpaths, anchors)| {
        subpaths <= MAX_SUBPATHS
            && anchors <= MAX_PATH_ANCHORS
            && bake.samples(canvas) <= MAX_BAKE_SAMPLES
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

/// [`vmsk_coverage`] with no job: the seam a black-box test reads the drawn
/// u16 samples through (an 8-bit open rounds them to bytes).
#[doc(hidden)]
pub fn vmsk_bake(body: &[u8], canvas: (u32, u32)) -> Option<Result<Vec<u16>, String>> {
    vmsk_coverage(body, canvas, crate::job_control::JobContext::NONE)
}

/// The coverage over the whole `canvas` of a `vmsk` body Doctored does not
/// hold as a live vector mask, under its per-subpath operations, initial
/// fill and inverted flag (`vector_mask_combine`), each subpath drawn over
/// its own window (module doc), for an import that keeps the picture as a
/// raster mask. `None` for a body that does not decode, names an unknown
/// operation or is disabled (flag bit 2: it masks nothing); `Some(Err)` is
/// the rasterizer's refusal or a cancellation.
pub(crate) fn vmsk_coverage(
    body: &[u8],
    canvas: (u32, u32),
    job: crate::job_control::JobContext<'_>,
) -> Option<Result<Vec<u16>, String>> {
    let (bake, _, _) = Bake::read(body, canvas)?;
    let mut acc = Accumulator::start(canvas.0, canvas.1, bake.full);
    for (one, op) in &bake.subpaths {
        // Each side of a window is within the canvas, at most 300,000 px.
        let drawn = acc.apply(*op, hull(one, canvas), |window| {
            one.coverage(
                (window.x as i32, window.y as i32),
                window.width,
                window.height,
                true,
                job,
            )
        });
        if let Err(reason) = drawn {
            return Some(Err(reason));
        }
    }
    Some(Ok(acc.finish(bake.invert)))
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
        // accumulator and the mask the layer keeps, two u16 planes. A
        // subpath is drawn over its own window, at most the canvas, so the
        // canvas-sized charge is an upper bound.
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
