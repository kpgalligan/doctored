//! COLOUR SAMPLERS — up to ten pinned canvas pixels
//! the Info panel reads live, on the same shelf as the guides
//! (`doc_guide`): canvas-space document state that never composites, is
//! saved in `.rz` and PSD (image resource 1073), and is moved by every
//! canvas-geometry op the way the pixels move.
//!
//! **Coordinate convention.** A sampler is a PIXEL INDEX, not a grid line:
//! `(x, y)` names the pixel whose top-left corner is `(x, y)`, so the legal
//! range is `[0, width) × [0, height)` — exclusive at the far edges, unlike a
//! guide. `sample_size` is the side of the averaged square, 1, 3 or 5.
//!
//! **The geometry rule, one for every op.** The pixel CENTRE `(x + 0.5,
//! y + 0.5)` is mapped through the op's own canvas map and the result is
//! FLOORED back to a pixel index. For an integer permutation or shift that
//! is exact (a quarter turn of pixel `x` lands on pixel `H − 1 − y`, and so
//! on); for a scale or a filtered turn it picks the pixel the content under
//! the old centre now covers. A sampler whose mapped pixel lies outside the
//! new canvas is DROPPED; the list keeps its order, so the labels after a
//! drop renumber. Resize alone CLAMPS (a scale cannot move a centre off the
//! canvas; the clamp is float safety).
//!
//! **What moves them.** Quarter turns and flips ([`geometry_samplers`], the
//! maps `doc_guide::geometry_guides` uses), Crop ([`cropped_samplers`]),
//! Canvas Size ([`padded_samplers`]), Image Size ([`resized_samplers`]) and
//! the two filtered whole-canvas turns ([`matrix_samplers`]): Rotate Canvas
//! by its `RotatedCanvas::matrix` into the expanded canvas, and Straighten
//! by its canvas matrix then the crop window. Unlike the guides, which only
//! SHIFT under a filtered turn (they are lines and cannot rotate), samplers
//! rotate with the content. A per-layer transform moves nothing about the
//! canvas, so it leaves them alone. [`edited_samplers`] is the one typed
//! dispatcher over a `GeometryEdit`; the byte ops call the helpers inside
//! their own struct literals, exactly as they do the guide helpers.
//!
//! The cap [`MAX_COLOR_SAMPLERS`] has ONE home here: `.rz` (`sampler_native`)
//! and PSD (`psd_samplers`) both read it, and the host's `ColorSamplerSet`
//! limit names it.
//!
//! `DrDocument.color_samplers` / `PrecisionDocument.color_samplers`, at most `MAX_COLOR_SAMPLERS`
//! (10, its one home) PIXEL INDICES with a 1/3/5 sample size, set whole by the pure
//! `with_color_samplers` (`validated`; an invalid or identical list is `None`) through
//! `dr_doc_with_color_samplers` / `dr_precision_doc_with_color_samplers` and read by the
//! count/triple getters. Every whole-canvas geometry op maps each pixel CENTRE through its canvas
//! map and floors it, DROPPING a sampler that leaves the canvas (order kept): the byte ops call
//! `geometry_samplers` / `cropped_samplers` / `padded_samplers` / `resized_samplers` (clamps) in
//! their struct literals, the filtered `rotate_canvas` and the byte Straighten arm
//! `matrix_samplers` (samplers ROTATE where guides shift), and the typed fold asks
//! `edited_samplers` once and counts it toward `changed`; `exact_geometry_changed` compares them;
//! every `ruler_origin` carry site carries them (a duplicate-to-new-document and a PSD-built
//! document start empty). `flat_save_losses` names `"color samplers"`. Tests:
//! `tests/color_samplers.rs`.

use crate::doc::{DrDocument, Geometry};
use crate::doc_transform::Affine;
use crate::precision_geometry::GeometryEdit;

/// The largest sampler list a document may carry.
pub const MAX_COLOR_SAMPLERS: usize = 10;

/// One pinned pixel. Equality is value equality: two samplers at the same
/// pixel and size are the same sampler (they carry no identity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorSampler {
    pub x: u32,
    pub y: u32,
    /// The side of the averaged square: 1, 3 or 5.
    pub sample_size: u8,
}

/// Whether `size` is a sample size a sampler may carry.
pub(crate) fn valid_size(size: u8) -> bool {
    matches!(size, 1 | 3 | 5)
}

/// `list` as a document may hold it on a `w × h` canvas, or `None` for more
/// than [`MAX_COLOR_SAMPLERS`] entries, an entry off the canvas or a sample
/// size other than 1, 3 or 5. Never a repair: a caller asked for a specific
/// list and is told no.
pub(crate) fn validated(list: &[ColorSampler], w: u32, h: u32) -> Option<Vec<ColorSampler>> {
    if list.len() > MAX_COLOR_SAMPLERS {
        return None;
    }
    list.iter()
        .all(|s| s.x < w && s.y < h && valid_size(s.sample_size))
        .then(|| list.to_vec())
}

/// The shared body of every DROPPING helper: maps each pixel centre through
/// `map`, floors it and keeps the samplers whose pixel lies in
/// `[0, w) × [0, h)` — a non-finite answer is dropped too.
pub(crate) fn mapped_dropping(
    s: &[ColorSampler],
    w: u32,
    h: u32,
    map: impl Fn(f64, f64) -> (f64, f64),
) -> Vec<ColorSampler> {
    s.iter()
        .filter_map(|sampler| {
            let (px, py) = map(f64::from(sampler.x) + 0.5, f64::from(sampler.y) + 0.5);
            let (fx, fy) = (px.floor(), py.floor());
            let inside = fx.is_finite()
                && fy.is_finite()
                && (0.0..f64::from(w)).contains(&fx)
                && (0.0..f64::from(h)).contains(&fy);
            inside.then_some(ColorSampler {
                x: fx as u32,
                y: fy as u32,
                ..*sampler
            })
        })
        .collect()
}

/// Every sampler through one exact whole-document transform. `w` and `h`
/// are the OLD canvas. The five maps are `doc_guide::geometry_guides`':
/// R90 `(H − cy, cx)`, R180 `(W − cx, H − cy)`, R270 `(cy, W − cx)`, FlipH
/// `(W − cx, cy)`, FlipV `(cx, H − cy)`. A permutation keeps every sampler,
/// so the drop never fires here.
pub(crate) fn geometry_samplers(
    s: &[ColorSampler],
    geom: Geometry,
    w: u32,
    h: u32,
) -> Vec<ColorSampler> {
    let (fw, fh) = (f64::from(w), f64::from(h));
    let (new_w, new_h) = if geom.swaps_axes() { (h, w) } else { (w, h) };
    mapped_dropping(s, new_w, new_h, |cx, cy| match geom {
        Geometry::Rotate90 => (fh - cy, cx),
        Geometry::Rotate180 => (fw - cx, fh - cy),
        Geometry::Rotate270 => (cy, fw - cx),
        Geometry::FlipH => (fw - cx, cy),
        Geometry::FlipV => (cx, fh - cy),
    })
}

/// Crop: shifts by `(−x, −y)` and DROPS what left the `w × h` window.
pub(crate) fn cropped_samplers(
    s: &[ColorSampler],
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Vec<ColorSampler> {
    let (dx, dy) = (f64::from(x), f64::from(y));
    mapped_dropping(s, w, h, |cx, cy| (cx - dx, cy - dy))
}

/// Canvas Size: shifts by `origin` — where the old canvas's top-left lands
/// in the new `w × h` one — and DROPS what left it.
pub(crate) fn padded_samplers(
    s: &[ColorSampler],
    w: u32,
    h: u32,
    origin: (i32, i32),
) -> Vec<ColorSampler> {
    let (dx, dy) = (f64::from(origin.0), f64::from(origin.1));
    mapped_dropping(s, w, h, |cx, cy| (cx + dx, cy + dy))
}

/// Image Size: scales each centre by the canvas's own factors and CLAMPS
/// into the new `w × h` canvas. Nothing can leave the canvas under a scale,
/// so nothing is dropped; the clamp is float safety only.
pub(crate) fn resized_samplers(
    s: &[ColorSampler],
    fx: f64,
    fy: f64,
    w: u32,
    h: u32,
) -> Vec<ColorSampler> {
    let clamp = |v: f64, extent: u32| -> u32 {
        let top = f64::from(extent.saturating_sub(1));
        if v.is_finite() {
            v.floor().clamp(0.0, top) as u32
        } else {
            0
        }
    };
    s.iter()
        .map(|sampler| ColorSampler {
            x: clamp((f64::from(sampler.x) + 0.5) * fx, w),
            y: clamp((f64::from(sampler.y) + 0.5) * fy, h),
            ..*sampler
        })
        .collect()
}

/// A filtered whole-canvas turn: each centre through the canvas matrix `m`
/// (`[a, b, c, d, tx, ty]`, `Affine::apply`'s convention), then into the
/// `window` `(x, y, w, h)` of the matrix's output canvas, DROPPING what
/// lands outside it. Rotate Canvas passes its `RotatedCanvas::matrix` with
/// the whole expanded canvas as the window; Straighten its matrix and its
/// crop window.
pub(crate) fn matrix_samplers(
    s: &[ColorSampler],
    m: [f64; 6],
    window: (i32, i32, u32, u32),
) -> Vec<ColorSampler> {
    let affine = Affine::from_array(m);
    let (wx, wy) = (f64::from(window.0), f64::from(window.1));
    mapped_dropping(s, window.2, window.3, |cx, cy| {
        let (px, py) = affine.apply(cx, cy);
        (px - wx, py - wy)
    })
}

/// The samplers after `edit` on a `w × h` canvas, as the byte depth moves
/// them — the typed geometry fold's one call, and the byte Straighten
/// arm's. A per-layer transform leaves them alone; Rotate Canvas delegates
/// a quarter turn to [`geometry_samplers`] exactly as the op delegates to
/// `rotate90()` and friends, and is the identity for a whole turn.
pub(crate) fn edited_samplers(
    s: &[ColorSampler],
    edit: &GeometryEdit,
    w: u32,
    h: u32,
) -> Result<Vec<ColorSampler>, String> {
    use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
    Ok(match edit {
        GeometryEdit::Rotate90 => geometry_samplers(s, Geometry::Rotate90, w, h),
        GeometryEdit::Rotate180 => geometry_samplers(s, Geometry::Rotate180, w, h),
        GeometryEdit::Rotate270 => geometry_samplers(s, Geometry::Rotate270, w, h),
        GeometryEdit::FlipHorizontal => geometry_samplers(s, Geometry::FlipH, w, h),
        GeometryEdit::FlipVertical => geometry_samplers(s, Geometry::FlipV, w, h),
        GeometryEdit::Resize { width, height, .. } => resized_samplers(
            s,
            f64::from(*width) / f64::from(w),
            f64::from(*height) / f64::from(h),
            *width,
            *height,
        ),
        GeometryEdit::Crop {
            x,
            y,
            width,
            height,
        } => cropped_samplers(s, *x, *y, *width, *height),
        GeometryEdit::CanvasSize {
            width,
            height,
            x,
            y,
        } => padded_samplers(s, *width, *height, (*x, *y)),
        GeometryEdit::Straighten {
            matrix,
            x,
            y,
            width,
            height,
            ..
        } => matrix_samplers(s, *matrix, (*x, *y, *width, *height)),
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, normalized) =
                rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
            match kind {
                RotationKind::Identity => s.to_vec(),
                RotationKind::Rotate90 => geometry_samplers(s, Geometry::Rotate90, w, h),
                RotationKind::Rotate180 => geometry_samplers(s, Geometry::Rotate180, w, h),
                RotationKind::Rotate270 => geometry_samplers(s, Geometry::Rotate270, w, h),
                RotationKind::Filtered => {
                    let plan = rotated_canvas(w, h, normalized)
                        .ok_or("the rotated canvas would exceed the supported canvas size")?;
                    matrix_samplers(s, plan.matrix, (0, 0, plan.width, plan.height))
                }
            }
        }
        GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::AffineSet { .. }
        | GeometryEdit::PerspectiveSet { .. } => s.to_vec(),
    })
}

impl DrDocument {
    /// The document's colour samplers, in label order (#1 first).
    pub fn color_samplers(&self) -> &[ColorSampler] {
        &self.color_samplers
    }

    /// The same document holding `list` instead. `None` — never an identical
    /// copy — when [`validated`] refuses the list or it equals the current
    /// one.
    pub fn with_color_samplers(&self, list: &[ColorSampler]) -> Option<Self> {
        let list = validated(list, self.width, self.height)?;
        if list == self.color_samplers {
            return None;
        }
        Some(Self {
            color_samplers: list,
            ..self.clone()
        })
    }
}
