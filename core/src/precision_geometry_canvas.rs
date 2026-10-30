//! Typed CANVAS SIZE and typed STRAIGHTEN (Phase 12F-1 / 12F-1b2). A `#[path]` submodule of
//! `precision_geometry`, beside `plan`: `precision_geometry_plan` is the
//! per-plane planner and this module is the per-OP rule those questions need,
//! so neither file states the other's job.
//!
//! # G1 — Canvas Size is a generalized Crop
//!
//! The byte contract is `DrDocument::canvas_resize` and it is the
//! specification; nothing about what a padded plane, a moved guide or a moved
//! channel MEANS is written twice. `(x, y)` is where the OLD canvas's top-left
//! corner lands in the new one — the byte `origin`.
//!
//! * every raster and every mask is [`WorkKind::Keep`] with its offset moved
//!   by the origin. The SAME allocation comes out: no sample is read, and the
//!   `Arc` a retained source's binding guard compares is preserved by
//!   construction (G5);
//! * every saved CHANNEL becomes a new canvas-sized plane holding an exact
//!   copy of the old one at the origin and 0 elsewhere — `doc_channel::
//!   padded_plane`'s rule, expressed through the existing Nearest plane walk
//!   with `outside = 0`, which is the same map;
//! * the chrome is `doc_guide::padded_guides` / `padded_origin`
//!   (`precision_guide::geometry_chrome`'s `CanvasSize` arm — those two
//!   helpers' FIRST typed caller);
//! * lock-EXEMPT, like every other whole-canvas op (`precision_geometry`'s
//!   gate doc), style factor 1, no `.rz` change and no new FFI export: it
//!   rides `dr_precision_doc_edit` as one more `GeometryEdit`.
//!
//! ONE deliberate difference from the byte op, an existing typed contract
//! rather than a new rule: an offset that would leave `i32` REFUSES
//! (`rounded_offset`'s sentence) where the byte op saturates.
//!
//! The exact no-op used to be a second one, and since Phase 12F-1b it is not:
//! `DrDocument::canvas_resize` answers `None` for the same size at origin
//! `(0, 0)` too, where it used to answer an identical copy and register a
//! phantom undo step. Both depths now follow the one purity rule — an op that
//! changes nothing returns `None` — and the HOST answers that request as a
//! SUCCESSFUL no-op (`changed: false`, no step, no dirtying) at 8 bits, at 16
//! and at float alike, before it ever calls either.
//!
//! # G6 — Straighten is one op, not a composition
//!
//! The byte contract is byte `geometry_edit`'s Straighten arm and it is the
//! specification: `doc_align::straighten_layers(m, filter)`, then
//! `doc_channel::transform_channels(m, filter)`, then `doc::crop(x, y, w, h)`.
//! [`GeometryEdit::Straighten`] is the ONE typed op equal to that chain, so a
//! typed host makes one call inside one undo step and no rule is written
//! twice.
//!
//! ONE PLAN ([`straighten_plane`]). Every `Content::Raster` and EVERY mask —
//! linked, unlinked, on a group, on an adjustment — is
//! `plan::affine(source, T(−x, −y) ∘ m, filter, channels)`, where `T(−x, −y) ∘ m`
//! is `m` with `tx −= x, ty −= y`. That integer shift COMMUTES with
//! `bounds_of_corners`' snap/floor/ceil, so each destination extent is
//! exactly the byte "transform, then crop" answer; nothing is trimmed,
//! because the byte `crop` retains off-canvas storage too. `exact_form` still
//! pins the filter to Nearest and still collapses an identity or an integer
//! translation to [`WorkKind::Keep`], so a quarter-turn matrix reaching this
//! op is a permutation rather than a resample.
//!
//! Every saved CHANNEL ([`straighten_channel`]) is resampled STRAIGHT from
//! the old canvas into `(0, 0, width, height)` with that same inverse — which
//! is `transform_channels` followed by `cropped_channels`, the window being
//! inside the canvas — with `outside = 0`, exactly as the byte channel walk
//! reads 0 where there is no source. `Keep` only for the identity at the full
//! window.
//!
//! The rest of the rule is one sentence each: the chrome is `cropped_*`
//! (`precision_guide::geometry_chrome`'s Straighten arm), the PPI is NEVER
//! transposed, the Scale Effects factor is `sqrt(|det m|)` on RASTER records
//! only (the byte twin is `transform_layer_unlocked_with_job`), the op is lock-EXEMPT
//! like every other whole-canvas op, a resampled raster DROPS its
//! description and a retained source REFUSES through `geometry_plan`'s
//! existing `unsupported` arm.
//!
//! The window may lie past any canvas edge, wherever
//! `crop_background::window_fits` admits it ([`straighten_window`]): the
//! planes above already map into any window, a channel reads 0 where the old
//! canvas had none, and the chrome is `padded_*` by `(−x, −y)`. The byte arm
//! takes the same window (`doc_source_geometry`), and a Background (Default)
//! fill pads a qualifying record 0 first at both depths (`doc_crop_extend`).
//!
//! Phase 12F-1b4 adds strict `RotateCanvas { degrees, filter }` at 16 bits through the same
//! `doc_rotate::{rotation_kind, rotated_canvas}` as byte depth: identity is `None`, exact quarters
//! reuse existing plans/PPI/chrome, and filtered rotation maps every raster, EVERY mask and every
//! saved channel directly into the expanded canvas, with padded chrome, style factor 1 and cleared
//! resampled descriptions. Straighten and RotateCanvas share the canvas dispatcher.
//!
//! # Delete Cropped Pixels
//!
//! A Straighten with `delete_cropped` clips each ELIGIBLE raster ([`trims`]: no description, no
//! retained source, no warp — the byte `doc_delete_cropped` rule) to the new canvas
//! ([`deleting_cropped`]). Only the destination changes: a clipped `Keep` plane becomes the
//! windowed translation at Nearest (a bit copy of its window), a resampled plane keeps its inverse
//! and the user's filter, and a plane wholly outside becomes a 1×1 transparent pixel at (0, 0)
//! sampled at Nearest (its one pixel centre lies outside the transformed layer, so it reads
//! nothing). Each clipped plane is rebuilt through `plan::work`, so the plan charges its output
//! and work at the clipped size. Masks, channels and every other record are as above.

use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
use crate::doc_transform::{exact_form, Affine, Exact};
use crate::precision_sampling::GeometryFilter;

use super::plan::{affine, rounded_offset, work, Extent, PlaneWork, WorkKind};
use super::GeometryEdit;
use crate::precision_document::PrecisionDocumentLayer;

/// The canvas result for the two filtered whole-canvas operations. Exact
/// RotateCanvas angles deliberately reuse the existing quarter-turn sizes.
pub(super) fn window(
    canvas: (u32, u32),
    edit: &GeometryEdit,
    channels: usize,
) -> Result<(u32, u32), String> {
    match edit {
        GeometryEdit::Straighten {
            matrix,
            x,
            y,
            width,
            height,
            ..
        } => straighten_window(*matrix, *x, *y, *width, *height, channels),
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, normalized) =
                rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
            match kind {
                RotationKind::Identity | RotationKind::Rotate180 => Ok(canvas),
                RotationKind::Rotate90 | RotationKind::Rotate270 => Ok((canvas.1, canvas.0)),
                RotationKind::Filtered => {
                    let plan = rotated_canvas(canvas.0, canvas.1, normalized)
                        .ok_or("the rotated canvas would exceed the supported canvas size")?;
                    Ok((plan.width, plan.height))
                }
            }
        }
        _ => Err("invalid filtered canvas geometry request".into()),
    }
}

fn rotation_matrix(canvas: (u32, u32), degrees: f64) -> Result<Affine, String> {
    let (kind, normalized) = rotation_kind(degrees).ok_or("rotation degrees must be finite")?;
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    let matrix = match kind {
        RotationKind::Identity => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        RotationKind::Rotate90 => [0.0, 1.0, -1.0, 0.0, h, 0.0],
        RotationKind::Rotate180 => [-1.0, 0.0, 0.0, -1.0, w, h],
        RotationKind::Rotate270 => [0.0, -1.0, 1.0, 0.0, 0.0, w],
        RotationKind::Filtered => {
            rotated_canvas(canvas.0, canvas.1, normalized)
                .ok_or("the rotated canvas would exceed the supported canvas size")?
                .matrix
        }
    };
    Ok(Affine::from_array(matrix))
}

/// Per-raster/mask dispatch for Straighten and RotateCanvas.
pub(super) fn plane(
    source: Extent,
    canvas: (u32, u32),
    edit: &GeometryEdit,
    channels: u64,
) -> Result<PlaneWork, String> {
    match edit {
        GeometryEdit::Straighten {
            matrix,
            filter,
            x,
            y,
            ..
        } => straighten_plane(source, *matrix, *filter, *x, *y, channels),
        GeometryEdit::RotateCanvas { degrees, filter } => affine(
            source,
            rotation_matrix(canvas, *degrees)?,
            *filter,
            channels,
        ),
        _ => Err("invalid filtered canvas geometry request".into()),
    }
}

/// Per-channel dispatch for Straighten and RotateCanvas. Channels always map
/// directly from the old canvas into the final canvas with outside zero.
pub(super) fn channel(
    canvas: (u32, u32),
    destination: (u32, u32),
    edit: &GeometryEdit,
) -> Result<PlaneWork, String> {
    match edit {
        GeometryEdit::Straighten {
            matrix,
            filter,
            x,
            y,
            ..
        } => straighten_channel(
            canvas,
            (*x, *y, destination.0, destination.1),
            *matrix,
            *filter,
        ),
        GeometryEdit::RotateCanvas { degrees, filter } => {
            let matrix = rotation_matrix(canvas, *degrees)?;
            let inverse = matrix.inverse().ok_or("typed affine matrix is singular")?;
            let exact = exact_form(&matrix);
            let identity = matches!(exact, Some(Exact::Translate))
                && destination == canvas
                && matrix.tx.round() == 0.0
                && matrix.ty.round() == 0.0;
            let kind = if identity {
                WorkKind::Keep
            } else {
                WorkKind::Affine {
                    inverse,
                    filter: if exact.is_some() {
                        GeometryFilter::Nearest
                    } else {
                        *filter
                    },
                }
            };
            work(
                (0, 0, canvas.0, canvas.1),
                (0, 0, destination.0, destination.1),
                kind,
                1,
            )
        }
        _ => Err("invalid filtered canvas geometry request".into()),
    }
}

/// G1's per-plane work for ONE raster or mask: the same allocation, its
/// offset moved by the origin. `plan::build`'s `whole` dispatches here.
pub(super) fn size_plane(
    source: Extent,
    x: i32,
    y: i32,
    channels: u64,
) -> Result<PlaneWork, String> {
    let dest = (
        rounded_offset(f64::from(source.0) + f64::from(x))?,
        rounded_offset(f64::from(source.1) + f64::from(y))?,
        source.2,
        source.3,
    );
    work(source, dest, WorkKind::Keep, channels)
}

/// G1's per-plane work for ONE saved channel: a new canvas-sized plane
/// holding an exact copy of the old canvas at the origin and 0 elsewhere.
/// `Keep` only when nothing at all moved, so the identity shares its
/// allocation like every other exact typed op.
pub(super) fn size_channel(
    source: Extent,
    canvas: (u32, u32),
    new_canvas: (u32, u32),
    x: i32,
    y: i32,
) -> Result<PlaneWork, String> {
    let kind = if x == 0 && y == 0 && new_canvas == canvas {
        WorkKind::Keep
    } else {
        WorkKind::Affine {
            inverse: Affine::from_array([1.0, 0.0, 0.0, 1.0, -f64::from(x), -f64::from(y)]),
            filter: GeometryFilter::Nearest,
        }
    };
    work(source, (0, 0, new_canvas.0, new_canvas.1), kind, 1)
}

/// `m` composed with the translation that moves the straighten's window to
/// the new canvas origin: `T(−x, −y) ∘ m`, i.e. `m` with `tx −= x, ty −= y`.
/// The ONE place that composition is written (G6).
fn windowed(matrix: [f64; 6], x: i32, y: i32) -> Affine {
    Affine::from_array([
        matrix[0],
        matrix[1],
        matrix[2],
        matrix[3],
        matrix[4] - f64::from(x),
        matrix[5] - f64::from(y),
    ])
}

/// G6's canvas answer: the straighten's window IS the new canvas, anywhere
/// `crop_background::window_fits` admits it. `plan::build`'s canvas
/// match dispatches here.
///
/// The matrix is validated first so a caller sending both a bad matrix and a
/// bad window is told about the matrix, exactly as `plan::affine` would have.
/// A zero-size window answers `pixel_count`'s own sentence, which is what
/// every other whole-document op answers for one.
pub(super) fn straighten_window(
    matrix: [f64; 6],
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    channels: usize,
) -> Result<(u32, u32), String> {
    let m = Affine::from_array(matrix);
    if !m.is_finite() {
        return Err("typed affine matrix must be finite".into());
    }
    if m.inverse().is_none() {
        return Err("typed affine matrix is singular".into());
    }
    crate::crop_background::window_fits(x, y, width, height, channels)?;
    Ok((width, height))
}

/// G6's per-plane work for ONE raster or mask: the shipped `plan::affine`
/// under `T(−x, −y) ∘ m`. The integer shift commutes with the extent's
/// snap/floor/ceil, so this IS the byte "transform, then crop" destination.
pub(super) fn straighten_plane(
    source: Extent,
    matrix: [f64; 6],
    filter: GeometryFilter,
    x: i32,
    y: i32,
    channels: u64,
) -> Result<PlaneWork, String> {
    affine(source, windowed(matrix, x, y), filter, channels)
}

/// G6's per-plane work for ONE saved channel: resampled STRAIGHT from the old
/// canvas into the window, which is `transform_channels` then
/// `cropped_channels` with no intermediate plane. `Keep` only for the identity
/// at the full window — any other map really does move samples.
pub(super) fn straighten_channel(
    canvas: (u32, u32),
    window: (i32, i32, u32, u32),
    matrix: [f64; 6],
    filter: GeometryFilter,
) -> Result<PlaneWork, String> {
    let (x, y, width, height) = window;
    let m = windowed(matrix, x, y);
    // No finiteness check: `straighten_window` already refused a non-finite
    // AND a singular matrix before `plan::build` reached any plane, so the
    // `inverse()` below can only fail on a matrix that was already rejected.
    // It is kept rather than unwrapped because a `?` on an Option is the
    // cheapest possible backstop and this function has no other way to say no.
    let inverse = m.inverse().ok_or("typed affine matrix is singular")?;
    let exact = exact_form(&m);
    let identity = matches!(exact, Some(Exact::Translate))
        && m.tx.round() == 0.0
        && m.ty.round() == 0.0
        && (width, height) == canvas;
    let kind = if identity {
        WorkKind::Keep
    } else {
        WorkKind::Affine {
            inverse,
            filter: if exact.is_some() {
                GeometryFilter::Nearest
            } else {
                filter
            },
        }
    };
    work((0, 0, canvas.0, canvas.1), (0, 0, width, height), kind, 1)
}

/// Whether Delete Cropped Pixels clips `layer`: a raster with no
/// description and no retained source or warp — `doc_delete_cropped`'s rule.
pub(crate) fn trims(layer: &PrecisionDocumentLayer) -> bool {
    layer.is_pixel_leaf()
        && layer.description.is_none()
        && layer.retained_source.is_none()
        && layer.warp.is_none()
}

/// An eligible raster's Straighten plane under Delete Cropped Pixels (the
/// module doc's rule): `plane` unchanged unless the edit asks for it and
/// the plane reaches outside `(0, 0, width, height)`. The plan records how
/// far the destination's corner moved (`LayerWork::anchor_shift`), which a
/// linked pattern overlay's anchor is rebased by.
pub(super) fn deleting_cropped(
    plane: PlaneWork,
    edit: &GeometryEdit,
    channels: u64,
) -> Result<PlaneWork, String> {
    let GeometryEdit::Straighten {
        matrix,
        x,
        y,
        width,
        height,
        delete_cropped: true,
        ..
    } = edit
    else {
        return Ok(plane);
    };
    let (dx, dy, dw, dh) = plane.dest;
    let (left, top) = (i64::from(dx), i64::from(dy));
    let (right, bottom) = (left + i64::from(dw), top + i64::from(dh));
    let (x0, y0) = (left.max(0), top.max(0));
    let (x1, y1) = (right.min(i64::from(*width)), bottom.min(i64::from(*height)));
    if (x0, y0, x1, y1) == (left, top, right, bottom) {
        return Ok(plane);
    }
    let inverse = windowed(*matrix, *x, *y)
        .inverse()
        .ok_or("typed affine matrix is singular")?;
    let (dest, kind) = if x0 < x1 && y0 < y1 {
        let dest = (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
        let kind = match plane.kind {
            WorkKind::Keep => WorkKind::Affine {
                inverse,
                filter: GeometryFilter::Nearest,
            },
            other => other,
        };
        (dest, kind)
    } else {
        (
            (0, 0, 1, 1),
            WorkKind::Affine {
                inverse,
                filter: GeometryFilter::Nearest,
            },
        )
    };
    work(plane.source, dest, kind, channels)
}
