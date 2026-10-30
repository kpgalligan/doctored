//! Free transform: one layer resampled through an arbitrary affine matrix.
//!
//! Conventions, all fixed by [`DrDocument::transform_layer`] and restated in
//! the C header:
//!
//! - The matrix works in CANVAS coordinates. A layer occupies the canvas rect
//!   `offset .. offset + pixel dimensions`; the matrix says where that rect
//!   lands, and the new offset/size fall out of the transformed corners'
//!   bounding box (rounded outward, so nothing is clipped).
//! - Resampling is inverse-mapped: every DESTINATION pixel centre is mapped
//!   back through the inverse matrix and sampled in the source. Samples that
//!   fall outside the source pixels are fully transparent, which is also what
//!   makes the edges fade out instead of stopping abruptly.
//! - Interpolation happens in PREMULTIPLIED f32 and is unpremultiplied on the
//!   way out. Interpolating straight RGBA would drag the (meaningless) color
//!   of fully transparent neighbours into edge pixels and fringe them.
//! - A linked mask follows the same matrix/kernel in its own independent
//!   extent. An unlinked mask retains its canvas location and raw pixels.
//!   Coverage uses the scalar sampler shared with selection transforms.

use std::sync::Arc;

use image::imageops::FilterType;
use image::{GrayImage, RgbaImage};

use crate::doc::{DrDocument, Geometry, Layer, LayerRaster, MAX_PIXELS};
use crate::doc_lock::EditKind;
use crate::job_control::JobContext;
use crate::reference_point::ReferenceOp;
use crate::sampling::{accumulate, quantize_coverage, straight, Kernel};
use crate::style::scaled_style;

/// Smallest determinant magnitude that still counts as invertible. Anything
/// smaller collapses the layer onto a line or a point, which cannot be
/// inverse-mapped (and whose bounding box would be empty anyway).
pub(crate) const MIN_DETERMINANT: f64 = 1e-9;

/// The share of a member's progress its RGBA raster's row reports span when a
/// LINKED mask follows it. The mask resample reports no rows, and at
/// equal extents the raster's 4 bytes per pixel are 4/5 of the member's
/// samples, so progress holds at 0.8 while the mask resamples instead of
/// reaching the member's end with work still left.
pub(crate) const LINKED_MASK_RASTER_SHARE: f64 = 0.8;

/// How far a value may sit from an exact 0, +/-1 or integer and still be
/// treated as that exact value — used both to recognize the resampling-free
/// matrices and to keep a hair's-breadth overshoot from widening the
/// destination extent by a whole pixel.
///
/// Hosts do not hand over hand-written matrices; they COMPOSE them, and
/// `cos(PI/2)` is 6.123e-17 rather than 0. Comparing against exact zeros
/// would therefore never recognize a quarter turn built from an angle, and
/// the same 1e-17-scale residue turns a corner at 60 into 60.000000000000006,
/// whose ceil is 61 — a phantom, fully transparent column.
///
/// 1e-9 sits in the wide gap between the two error scales that matter:
///
/// - Above the noise: composing a rotation and a scale accumulates a few ULPs,
///   i.e. ~1e-16 RELATIVE error, so any coordinate up to ~1e7 — far larger
///   than any realistic canvas — still lands within 1e-9 of its true value.
///   A relative epsilon would buy nothing at these magnitudes and would make
///   the fast path depend on how far from the origin the layer sits; and past
///   1e7 an absolute epsilon simply stops matching, which costs a resample
///   (the behaviour before this tolerance existed) rather than correctness.
/// - Below anything a user can mean: the nearest angle to a right angle a UI
///   can express, 89.999 degrees, is 1.7e-5 radians off, which throws the
///   linear elements ~1.7e-5 off exact and the corners of even a 1-pixel layer
///   ~1e-5 off an integer — four orders of magnitude outside this epsilon, so
///   it still resamples. Nothing user-visible is snapped: a sub-nanopixel
///   difference cannot survive quantization to a u8 pixel grid.
///
/// It is also the tolerance the app's own identity check uses on a composed
/// matrix's linear part (`LayerTransform.isIdentity`), so the two agree on
/// what counts as "the same transform".
pub(crate) const EXACT_EPSILON: f64 = 1e-9;

/// True when `v` is within [`EXACT_EPSILON`] of `target`.
fn near(v: f64, target: f64) -> bool {
    (v - target).abs() <= EXACT_EPSILON
}

/// Snaps a coordinate sitting within [`EXACT_EPSILON`] of an integer onto that
/// integer. Applied to the transformed corners BEFORE the outward rounding, so
/// a composition's floating-point residue cannot inflate the extent.
pub(crate) fn snap_to_integer(v: f64) -> f64 {
    let rounded = v.round();
    if near(v, rounded) {
        rounded
    } else {
        v
    }
}

// ------------------------------------------------------------------ matrix --

/// A 2x3 affine matrix in CANVAS space, in `CGAffineTransform` element order:
/// `[a, b, c, d, tx, ty]` maps `(x, y)` to
/// `(a*x + c*y + tx, b*x + d*y + ty)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// x scale (row 0, column 0).
    pub a: f64,
    /// y shear (row 0, column 1).
    pub b: f64,
    /// x shear (row 1, column 0).
    pub c: f64,
    /// y scale (row 1, column 1).
    pub d: f64,
    /// x translation.
    pub tx: f64,
    /// y translation.
    pub ty: f64,
}

impl Affine {
    /// Reads the six elements in the order the FFI receives them
    /// (`[a, b, c, d, tx, ty]`).
    pub fn from_array(m: [f64; 6]) -> Self {
        Affine {
            a: m[0],
            b: m[1],
            c: m[2],
            d: m[3],
            tx: m[4],
            ty: m[5],
        }
    }

    pub(crate) fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.tx, self.ty]
            .iter()
            .all(|v| v.is_finite())
    }

    pub(crate) fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    pub(crate) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// The inverse map, or `None` when the matrix is singular (or so close to
    /// it that the inverse is meaningless).
    pub(crate) fn inverse(&self) -> Option<Affine> {
        let det = self.determinant();
        if !det.is_finite() || det.abs() < MIN_DETERMINANT {
            return None;
        }
        let inv = Affine {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            tx: (self.c * self.ty - self.d * self.tx) / det,
            ty: (self.b * self.tx - self.a * self.ty) / det,
        };
        if inv.is_finite() {
            Some(inv)
        } else {
            None
        }
    }
}

/// A matrix that needs no resampling at all.
#[derive(Clone, Copy)]
pub(crate) enum Exact {
    /// Integer translation only: the pixels are reused as they are.
    Translate,
    /// An integer translation composed with one of the five exact
    /// axis-aligned transforms, which the shared [`Geometry`] path applies to
    /// the pixels and the mask alike.
    Geom(Geometry),
}

/// Recognizes the matrices whose sampling would be a plain pixel copy: an
/// INTEGER translation (so destination pixel centres land exactly on source
/// pixel centres) combined with an axis-aligned linear part of 0s and +/-1s.
/// Both are matched within [`EXACT_EPSILON`], because a host composes these
/// matrices from an angle rather than writing them out, and the caller then
/// takes the destination extent from [`transformed_bounds`], which snaps the
/// same residue away. Anything else returns `None` and is resampled.
pub(crate) fn exact_form(m: &Affine) -> Option<Exact> {
    if !near(m.tx, m.tx.round()) || !near(m.ty, m.ty.round()) {
        return None;
    }
    // Linear parts as [a, b, c, d]. Rotate90 is the canvas-space map
    // (x, y) -> (-y, x), matching `imageops::rotate90` on a y-down canvas.
    let forms: [([f64; 4], Option<Geometry>); 6] = [
        ([1.0, 0.0, 0.0, 1.0], None),
        ([-1.0, 0.0, 0.0, 1.0], Some(Geometry::FlipH)),
        ([1.0, 0.0, 0.0, -1.0], Some(Geometry::FlipV)),
        ([-1.0, 0.0, 0.0, -1.0], Some(Geometry::Rotate180)),
        ([0.0, 1.0, -1.0, 0.0], Some(Geometry::Rotate90)),
        ([0.0, -1.0, 1.0, 0.0], Some(Geometry::Rotate270)),
    ];
    let linear = [m.a, m.b, m.c, m.d];
    forms
        .iter()
        .find(|(f, _)| f.iter().zip(linear).all(|(&want, got)| near(got, want)))
        .map(|(_, geom)| geom.map_or(Exact::Translate, Exact::Geom))
}

// ----------------------------------------------------------------- samplers --

/// Point sampling or one of the interpolating kernels, chosen by the caller's
/// `DrResizeFilter`.
#[derive(Clone, Copy)]
pub(crate) enum Sampler {
    /// Copies the covering source pixel's BYTES, straight alpha included.
    Nearest,
    Kernel(Kernel),
}

impl Sampler {
    /// Maps the shared resize-filter enum onto a transform sampler. The
    /// `image` crate's Gaussian filter has no `DrResizeFilter` value, so the
    /// FFI never asks for it; it is refused rather than quietly substituted.
    pub(crate) fn from_filter(filter: FilterType) -> Option<Self> {
        match filter {
            FilterType::Nearest => Some(Sampler::Nearest),
            FilterType::Triangle => Some(Sampler::Kernel(Kernel::Bilinear)),
            FilterType::CatmullRom => Some(Sampler::Kernel(Kernel::Bicubic)),
            FilterType::Lanczos3 => Some(Sampler::Kernel(Kernel::Lanczos3)),
            _ => None,
        }
    }
}

/// Maps destination pixel indices to source-LAYER coordinates: the inverse
/// map pre-composed with "destination pixel centre" on one side and the
/// layer's offset on the other, so `(u, v)` is a continuous coordinate in the
/// source layer's own pixel space.
///
/// The map is projective in general — numerators `nu`, `nv` and denominator
/// `nw`, each affine in the destination indices, with `(u, v) =
/// (nu, nv) / nw` — which is what lets `doc_perspective` reuse the two
/// resamplers below unchanged. An affine inverse is the `nw ≡ 1` special
/// case: its steps are zero, its origin is exactly 1.0, and `x / 1.0 == x`
/// in IEEE arithmetic, so the affine path's results are bit-identical to
/// what the plain two-component map produced. A destination pixel whose
/// denominator is not strictly positive lies on or beyond the horizon (the
/// vanishing line, where the inverse map flips to the mirror image behind
/// it) and samples as fully transparent; an affine map's constant 1 can
/// never trip that.
pub(crate) struct SourceMap {
    /// Per-destination-column step of `(nu, nv, nw)`.
    pub(crate) step: (f64, f64, f64),
    /// Per-destination-row step.
    row: (f64, f64, f64),
    /// The values at destination pixel (0, 0)'s centre.
    origin: (f64, f64, f64),
}

impl SourceMap {
    pub(crate) fn new(inv: &Affine, offset: (i32, i32), dest_origin: (i32, i32)) -> Self {
        let (u0, v0) = inv.apply(
            f64::from(dest_origin.0) + 0.5,
            f64::from(dest_origin.1) + 0.5,
        );
        SourceMap {
            step: (inv.a, inv.b, 0.0),
            row: (inv.c, inv.d, 0.0),
            origin: (u0 - f64::from(offset.0), v0 - f64::from(offset.1), 1.0),
        }
    }

    /// The projective twin of `new`: `g` is a row-major 3x3 INVERSE
    /// homography in canvas space (any overall scale, but oriented so the
    /// denominator is positive on the visible side of the horizon). The
    /// layer offset is folded into the numerators as `nu - offset * nw`,
    /// which keeps them affine in the indices and — with `nw ≡ 1` — is the
    /// very subtraction the affine constructor performs.
    pub(crate) fn projective(g: &[f64; 9], offset: (i32, i32), dest_origin: (i32, i32)) -> Self {
        let (ox, oy) = (f64::from(offset.0), f64::from(offset.1));
        let (x0, y0) = (
            f64::from(dest_origin.0) + 0.5,
            f64::from(dest_origin.1) + 0.5,
        );
        let at = |r: usize| g[3 * r] * x0 + g[3 * r + 1] * y0 + g[3 * r + 2];
        let (nu0, nv0, nw0) = (at(0), at(1), at(2));
        SourceMap {
            step: (g[0] - ox * g[6], g[3] - oy * g[6], g[6]),
            row: (g[1] - ox * g[7], g[4] - oy * g[7], g[7]),
            origin: (nu0 - ox * nw0, nv0 - oy * nw0, nw0),
        }
    }

    pub(crate) fn row_start(&self, j: u32) -> (f64, f64, f64) {
        (
            self.origin.0 + self.row.0 * f64::from(j),
            self.origin.1 + self.row.1 * f64::from(j),
            self.origin.2 + self.row.2 * f64::from(j),
        )
    }
}

/// Coordinates a source lookup may proceed with. A destination pixel whose
/// denominator is not strictly positive lies on or beyond the horizon and is
/// transparent by definition; one whose coordinates are wilder than any
/// layer can be (the pixel budget caps a side at 1e8) samples nothing
/// anyway, and skipping it keeps the huge values out of the tap index
/// arithmetic, whose `as`-saturation shields the CAST but whose subsequent
/// small additions must not be handed an already-saturated i64. NaN (from a
/// horizon-adjacent divide) fails the `<=` comparisons and is skipped too.
/// An affine map keeps `nw ≡ 1`, so the horizon half never trips for it;
/// the coordinate bound CAN trip on an accepted affine (a near-singular
/// anisotropic scale, det just over 1e-9, throws inverse coordinates past
/// 1e9), but every such coordinate sits at least an order of magnitude
/// outside any possible source and sampled as transparent before this
/// guard existed — affine results are unchanged either way.
pub(crate) fn visible(u: f64, v: f64, nw: f64) -> bool {
    nw > 0.0 && u.abs() <= 1e9 && v.abs() <= 1e9
}

/// Index of the source pixel covering `(u, v)`, or `None` outside the source.
pub(crate) fn covering_pixel(u: f64, v: f64, w: u32, h: u32) -> Option<usize> {
    let (x, y) = (u.floor(), v.floor());
    if x < 0.0 || y < 0.0 || x >= f64::from(w) || y >= f64::from(h) {
        return None;
    }
    Some(y as usize * w as usize + x as usize)
}

/// Inverse-maps `src` onto the destination extent, interpolating in
/// PREMULTIPLIED f32 and unpremultiplying on the way out. Destination pixels
/// whose source neighbourhood is entirely outside `src` — or that lie on or
/// beyond `map`'s horizon — come out fully transparent (all four bytes
/// zero). `dest` must be the extent `map` was built for.
pub(crate) fn resample_rgba(
    src: &RgbaImage,
    dest: (i32, i32, u32, u32),
    map: &SourceMap,
    sampler: Sampler,
) -> RgbaImage {
    resample_rgba_with_context(
        src,
        dest,
        map,
        sampler,
        false,
        crate::job_control::JobContext::NONE,
    )
    .expect("uncancellable context")
}

/// `report_progress` reports stage 6 once per destination row across `job`'s
/// range; a caller whose range is not the resample's own passes
/// `false`.
pub(crate) fn resample_rgba_with_context(
    src: &RgbaImage,
    dest: (i32, i32, u32, u32),
    map: &SourceMap,
    sampler: Sampler,
    report_progress: bool,
    job: crate::job_control::JobContext<'_>,
) -> Result<RgbaImage, String> {
    let (_, _, dw, dh) = dest;
    let raw = src.as_raw();
    let mut out = RgbaImage::new(dw, dh);
    let dst: &mut [u8] = &mut out;
    crate::sampling::resample_into(
        src.dimensions(),
        (dw, dh),
        map,
        sampler,
        |i| [raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]],
        |acc, px, weight| accumulate(acc, &px, weight),
        straight,
        |i, px| dst[i * 4..i * 4 + 4].copy_from_slice(&px),
        report_progress,
        false,
        job,
    )?;
    Ok(out)
}

/// The zero-background scalar twin of resample_rgba, for ordinary selections.
pub(crate) fn resample_mask_with_context(
    src: &GrayImage,
    dest: (i32, i32, u32, u32),
    map: &SourceMap,
    sampler: Sampler,
    control: crate::job_control::JobContext<'_>,
) -> Result<GrayImage, String> {
    resample_mask_outside(src, dest, map, sampler, 0, control)
}

/// Same scalar walk with a constant raw background. Subtracting the background
/// before tap accumulation preserves the shared zero-outside kernel; nearest
/// copies source samples exactly and untouched destination cells use background.
pub(crate) fn resample_mask_outside(
    src: &GrayImage,
    dest: (i32, i32, u32, u32),
    map: &SourceMap,
    sampler: Sampler,
    outside: u8,
    control: crate::job_control::JobContext<'_>,
) -> Result<GrayImage, String> {
    let (_, _, dw, dh) = dest;
    let raw = src.as_raw();
    let mut out = GrayImage::from_pixel(dw, dh, image::Luma([outside]));
    let dst: &mut [u8] = &mut out;
    crate::sampling::resample_into(
        src.dimensions(),
        (dw, dh),
        map,
        sampler,
        |i| raw[i],
        |acc: &mut [f32; 1], px, weight| acc[0] += weight * (f32::from(px) - f32::from(outside)),
        |acc| quantize_coverage(acc[0] + f32::from(outside)),
        |i, px| dst[i] = px,
        false,
        false,
        control,
    )?;
    Ok(out)
}

/// The whole-CANVAS twin of the per-layer resample: `src` is a canvas-space
/// coverage plane and so is the result. There is no bounding box, because a
/// canvas-space plane has nowhere to grow — every destination pixel centre is
/// inverse-mapped through `m` and sampled with `filter`'s kernel, and a
/// destination whose source lies off the plane reads 0, exactly as a layer
/// mask does under the same rotation.
///
/// It is the ONE resampling primitive behind
/// [`DrDocument::transform_channels`], which is why it lives here beside
/// `resample_mask` rather than growing a second inverse map of its own: a
/// channel and an identical layer mask carried through the same straighten
/// must come out byte for byte the same. `Err` for a non-finite or singular
/// matrix (`|det| < `[`MIN_DETERMINANT`]), a filter that is not one of the
/// four `DrResizeFilter` values, or `control`'s cancel.
pub(crate) fn resample_canvas_plane_with_context(
    src: &GrayImage,
    m: &Affine,
    filter: FilterType,
    control: crate::job_control::JobContext<'_>,
) -> Result<GrayImage, String> {
    if !m.is_finite() {
        return Err("selection transform matrix is not finite".into());
    }
    let sampler = Sampler::from_filter(filter).ok_or("unsupported selection transform filter")?;
    let inverse = m
        .inverse()
        .ok_or("selection transform matrix is singular")?;
    let (w, h) = src.dimensions();
    let map = SourceMap::new(&inverse, (0, 0), (0, 0));
    resample_mask_with_context(src, (0, 0, w, h), &map, sampler, control)
}

/// The destination placement: the axis-aligned bounding box of the layer
/// rect's four transformed corners, rounded OUTWARD (floor of the minima,
/// ceil of the maxima) so no source pixel is clipped. A corner within
/// [`EXACT_EPSILON`] of an integer is snapped onto it FIRST: without that, a
/// composed quarter turn whose corner lands on 60.000000000000006 would ceil
/// to 61 and grow a fully transparent column. Genuinely fractional corners are
/// untouched and still round outward. `None` when the box is not finite, does
/// not fit the i32 offset range, collapses to zero pixels, or exceeds
/// [`MAX_PIXELS`].
fn transformed_bounds(layer: &Layer, m: &Affine) -> Option<(i32, i32, u32, u32)> {
    let (lw, lh) = layer.pixels.dimensions();
    let (x0, y0) = (f64::from(layer.offset.0), f64::from(layer.offset.1));
    let (x1, y1) = (x0 + f64::from(lw), y0 + f64::from(lh));
    bounds_of_corners([
        m.apply(x0, y0),
        m.apply(x1, y0),
        m.apply(x0, y1),
        m.apply(x1, y1),
    ])
}

/// The corners' axis-aligned box `(left, top, right, bottom)`, each corner
/// snapped onto an integer within [`EXACT_EPSILON`] and the box then rounded
/// OUTWARD. `None` when a corner is not finite. [`bounds_of_corners`] adds the
/// range and size checks; `selected_pixels_transform` clips the box to the
/// canvas instead, so it needs the unchecked edges.
pub(crate) fn corner_box(corners: [(f64, f64); 4]) -> Option<(f64, f64, f64, f64)> {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in corners {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x, y) = (snap_to_integer(x), snap_to_integer(y));
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    Some((min.0.floor(), min.1.floor(), max.0.ceil(), max.1.ceil()))
}

/// The extent shared by both transform pipelines: `transformed_bounds` feeds
/// it the affinely mapped corners, `doc_perspective` the destination quad
/// itself (which IS the corners' image, so no mapping residue re-enters).
pub(crate) fn bounds_of_corners(corners: [(f64, f64); 4]) -> Option<(i32, i32, u32, u32)> {
    let (left, top, right, bottom) = corner_box(corners)?;
    if left < f64::from(i32::MIN)
        || top < f64::from(i32::MIN)
        || right > f64::from(i32::MAX)
        || bottom > f64::from(i32::MAX)
    {
        return None;
    }
    let (w, h) = ((right - left) as u64, (bottom - top) as u64);
    if w == 0 || h == 0 || w.checked_mul(h)? > MAX_PIXELS {
        return None;
    }
    Some((left as i32, top as i32, w as u32, h as u32))
}

impl DrDocument {
    /// Transforms layer `idx` by `m`, an affine matrix in CANVAS coordinates:
    /// the layer's rect (its offset plus its pixel dimensions) is mapped
    /// through `m`, and the layer's new offset and size come from the
    /// axis-aligned bounding box of the four transformed corners, rounded
    /// outward. Pixels are resampled by inverse mapping every destination
    /// pixel centre; samples outside the source are fully transparent.
    ///
    /// Only the named layer changes: the canvas is untouched (a transformed
    /// layer may extend past it, exactly like any other offset layer), as are
    /// every other layer and all of this layer's properties — name, opacity,
    /// blend mode, visibility, mask-enabled flag and `meta` (the core never
    /// interprets `meta`; deciding whether a transform invalidates it is the
    /// host's policy). An existing layer MASK is resampled with the same
    /// matrix and kernel in its own extent when linked; unlinked coverage is retained.
    ///
    /// Exact matrices — an integer translation optionally composed with a
    /// flip or a 90-degree multiple — skip resampling entirely and reuse the
    /// lossless pixel-copy paths, so they are byte-identical to the dedicated
    /// `rotate90`/`flip_*` ops. "Exact" is judged within [`EXACT_EPSILON`], so
    /// a quarter turn COMPOSED from an angle (where `cos` returns 6e-17
    /// instead of 0) qualifies too, and its extent is not inflated by that
    /// residue. A transform that is fractional by any user-meaningful amount
    /// resamples as usual.
    ///
    /// Returns `None` when `idx` is out of range, any matrix element is not
    /// finite, the matrix is singular (`|determinant| < 1e-9`), the filter is
    /// not one of the four `DrResizeFilter` values, or the destination extent
    /// is empty, outside the i32 offset range, or over [`MAX_PIXELS`].
    pub fn transform_layer(&self, idx: usize, m: Affine, filter: FilterType) -> Option<Self> {
        self.under_locks(idx, EditKind::Position, |doc| {
            doc.transform_layer_unlocked_with_job(idx, m, filter, JobContext::NONE, 0)
                .ok()
                .flatten()
        })
        .map(|out| self.following_transform(out, &m))
    }

    /// The follow hook of a layer transform by `m` (`doc_vector_mask`): a
    /// linked raster-less vector mask the transform resampled is rebuilt
    /// from its shape through `m`. An unlinked one kept its plane, and an
    /// exact translation only moved it; both are still live and are left
    /// alone.
    pub(crate) fn following_transform(&self, out: Self, m: &Affine) -> Self {
        self.following_vector_masks(out, |x, y| m.apply(x, y))
    }

    /// The body of [`Self::transform_layer`], outside the lock gate, polling
    /// `job` through the copy, the resample, the linked mask and a retained
    /// source's re-render (whose nested reservation keeps `floor`, the set's
    /// already-live buffers). Every domain refusal stays `Ok(None)`; `Err` is
    /// only ever the cancel — a failed sub-step that was not cancelled is
    /// today's `None`.
    ///
    /// The set caller decides the lock policy for a whole SET before it gets
    /// here: [`Self::transform_layers_with_job`] asks `movable_set`
    /// all-or-nothing first, and `straighten_layers` is exempt by design (a
    /// whole-document geometry op, like crop and resize). See `doc_align`.
    pub(crate) fn transform_layer_unlocked_with_job(
        &self,
        idx: usize,
        m: Affine,
        filter: FilterType,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        if layer.effective_source(&self.profile).is_some() {
            let filter = match filter {
                FilterType::Nearest => crate::precision_sampling::GeometryFilter::Nearest,
                FilterType::Triangle => crate::precision_sampling::GeometryFilter::Bilinear,
                FilterType::CatmullRom => crate::precision_sampling::GeometryFilter::Bicubic,
                FilterType::Lanczos3 => crate::precision_sampling::GeometryFilter::Lanczos3,
                _ => return Ok(None),
            };
            return self
                .source_edit_input_with_context(
                    None,
                    &crate::retained_source::SourceEdit::Transform {
                        layer: idx,
                        matrix: [m.a, m.b, m.c, m.d, m.tx, m.ty],
                        filter,
                        interactive: false,
                    },
                    crate::precision_image::process_working_limit(),
                    0,
                    job.with_reservation_floor(floor),
                )
                .map(|out| {
                    out.map(|mut doc| {
                        if let Some(target) = doc.layers.get_mut(idx) {
                            crate::doc_reference::follow_layer(
                                layer,
                                target,
                                ReferenceOp::Transform,
                                [m.a, m.b, m.c, m.d, m.tx, m.ty],
                                &crate::doc_reference::TurnedTiles::default(),
                            );
                        }
                        doc
                    })
                })
                .or_else(|e| cancel_or_refuse(job, e));
        }
        if !m.is_finite() {
            return Ok(None);
        }
        let Some(sampler) = Sampler::from_filter(filter) else {
            return Ok(None);
        };
        let Some(inverse) = m.inverse() else {
            return Ok(None);
        };
        let Some(dest) = transformed_bounds(layer, &m) else {
            return Ok(None);
        };
        if layer.mask_properties.linked && !self.mask_transform_fits(idx, &m) {
            return Ok(None);
        }
        let (dx, dy, dw, dh) = dest;
        if !self.native_layers_fit_dimensions(self.layers.iter().enumerate().map(|(i, l)| {
            if i == idx {
                (dw, dh)
            } else {
                l.pixels.dimensions()
            }
        })) {
            return Ok(None);
        }

        let pixels = match exact_form(&m) {
            Some(Exact::Translate) if layer.pixels.dimensions() == (dw, dh) => layer.pixels.clone(),
            Some(Exact::Geom(geom))
                if rotated_dims(layer.pixels.dimensions(), geom) == (dw, dh) =>
            {
                let image = match layer.pixels.to_contiguous_with_job(job) {
                    Ok(image) => image,
                    Err(e) => return cancel_or_refuse(job, e),
                };
                job.check()?;
                LayerRaster::unseeded(geom.apply(&image))
            }
            _ => {
                let image = match layer.pixels.to_contiguous_with_job(job) {
                    Ok(image) => image,
                    Err(e) => return cancel_or_refuse(job, e),
                };
                let map = SourceMap::new(&inverse, layer.offset, (dx, dy));
                let rows = if layer.mask.is_some() && layer.mask_properties.linked {
                    job.range(0.0, LINKED_MASK_RASTER_SHARE)
                } else {
                    job
                };
                match resample_rgba_with_context(&image, dest, &map, sampler, true, rows) {
                    Ok(pixels) => LayerRaster::unseeded(pixels),
                    Err(e) => return cancel_or_refuse(job, e),
                }
            }
        };
        let mut doc = self.clone();
        let Some(target) = doc.layers.get_mut(idx) else {
            return Ok(None);
        };
        if target.mask_properties.linked && target.affine_mask_with_job(m, filter, job)?.is_none() {
            return Ok(None);
        }
        target.pixels = pixels;
        target.offset = (dx, dy);
        // "Scale Effects": the style's pixel sizes follow the mean scale
        // sqrt(|det|) (1 for the exact paths, so their Arc is kept).
        target.style = scaled_style(&target.style, m.determinant().abs().sqrt());
        // The reference point and linked anchors follow M
        // (`doc_reference`); Free Transform keeps the tile.
        crate::doc_reference::follow_layer(
            layer,
            target,
            ReferenceOp::Transform,
            [m.a, m.b, m.c, m.d, m.tx, m.ty],
            &crate::doc_reference::TurnedTiles::default(),
        );
        Ok(Some(doc))
    }
}

/// A polled sub-step's failure under a job: the cancel stays an `Err`, any
/// other failure is the job-less path's refusal (`Ok(None)`, what `.ok()?`
/// answered before the step polled).
pub(crate) fn cancel_or_refuse<T>(job: JobContext<'_>, e: String) -> Result<Option<T>, String> {
    if job.cancelled() {
        Err(e)
    } else {
        Ok(None)
    }
}

/// The dimensions `geom` would produce from `img`.
fn rotated_dims((w, h): (u32, u32), geom: Geometry) -> (u32, u32) {
    match geom {
        Geometry::Rotate90 | Geometry::Rotate270 => (h, w),
        _ => (w, h),
    }
}

/// A resampled scalar plane and its canvas offset.
pub(crate) type PlacedGray = (Arc<GrayImage>, (i32, i32));

/// The mask twin of transform_layer, sharing bounds, exact copies and sampling.
pub(crate) fn transform_gray(
    src: &Arc<GrayImage>,
    offset: (i32, i32),
    m: Affine,
    filter: FilterType,
    outside: u8,
    job: JobContext<'_>,
) -> Result<Option<PlacedGray>, String> {
    if !m.is_finite() {
        return Ok(None);
    }
    let Some(inverse) = m.inverse() else {
        return Ok(None);
    };
    let Some(sampler) = Sampler::from_filter(filter) else {
        return Ok(None);
    };
    let Some(dest) = transformed_plane_bounds(src.dimensions(), offset, &m) else {
        return Ok(None);
    };
    let (dx, dy, dw, dh) = dest;
    let pixels = match exact_form(&m) {
        Some(Exact::Translate) if src.dimensions() == (dw, dh) => Arc::clone(src),
        Some(Exact::Geom(geom)) => Arc::new(geom.apply(&**src)),
        _ => match resample_mask_outside(
            src,
            dest,
            &SourceMap::new(&inverse, offset, (dx, dy)),
            sampler,
            outside,
            job,
        ) {
            Ok(mask) => Arc::new(mask),
            Err(e) => return cancel_or_refuse(job, e),
        },
    };
    Ok(Some((pixels, (dx, dy))))
}

/// Shared raw-plane extent preflight, before transform buffers are allocated.
pub(crate) fn transformed_plane_bounds(
    dims: (u32, u32),
    offset: (i32, i32),
    m: &Affine,
) -> Option<(i32, i32, u32, u32)> {
    if !m.is_finite() || m.inverse().is_none() {
        return None;
    }
    let (x, y) = (f64::from(offset.0), f64::from(offset.1));
    let (w, h) = (f64::from(dims.0), f64::from(dims.1));
    bounds_of_corners([
        m.apply(x, y),
        m.apply(x + w, y),
        m.apply(x, y + h),
        m.apply(x + w, y + h),
    ])
}
