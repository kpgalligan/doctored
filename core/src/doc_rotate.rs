//! DESTRUCTIVE whole-canvas rotation by ANY finite angle, at byte depth
//! (Phase 12F-1b3). One pure op with an EXPANDING canvas, and the single
//! place the classification of an angle and the size of the rotated canvas
//! are decided — the host writes neither rule, it asks
//! `dr_canvas_rotation_plan` for both.
//!
//! The 16-bit twin (`GeometryEdit::RotateCanvas`, Phase 12F-1b4) is planned
//! from [`rotation_kind`] and [`rotated_canvas`] too, so the two depths will
//! share one extent, one matrix and one exact-form decision rather than
//! agreeing by construction.
//!
//! # R1 — the matrix, and the ONE normalization
//!
//! Positive degrees turn the picture CLOCKWISE on a y-down canvas. The linear
//! part is `[cos, sin, −sin, cos]`, which is `CGAffineTransform(rotationAngle:)`,
//! is `rotate90`'s `[0, 1, −1, 0]` (`doc_transform::exact_form`), and is the
//! measured `R(θ)` the app's view rotation uses (`ViewRotation.swift`).
//!
//! The angle is NORMALIZED into `(−180, 180]` first — `−180` becomes `180`, a
//! `−0` becomes `0` — by `ViewRotation.normalize`'s formula written once more
//! here. It is returned to the host through the plan export so no caller
//! restates it.
//!
//! # R2 — Photoshop's ceil box, placed at the FLOOR of the centre
//!
//! The rotation is about the canvas CENTRE `(W/2, H/2)`. The new canvas is
//! Photoshop's `W′ = ceil(W·|cos| + H·|sin|)` × `H′ = ceil(W·|sin| + H·|cos|)`
//! (measured: every one of take1's nine rows), the extent of the four rotated
//! corners, each snapped by `doc_transform::snap_to_integer` and the extent
//! snapped once more. The old canvas's top-left lands at the INTEGER
//! `o = (⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)` — the centred placement floored, the
//! candidate nearest take1's content (`designs/phase-10c-constructs-status.md`)
//! — which is what lets the chrome ride `doc_guide::padded_guides` /
//! `padded_origin`, the very helpers Canvas Size uses, with no rule of their
//! own. Where `W′ − W` is odd the picture sits half a pixel left of (or above)
//! the new canvas's centre, and every sample keeps the resampling phase of
//! the old symmetric-box plan, a whole pixel away from this placement.
//!
//! `W′ < W` is ordinary, not an error: a 100×10 canvas at 80° is 28×101, and
//! its `left` is `+36`.
//!
//! # R3 — delegation is mandatory, not an optimisation
//!
//! Applying R2 to an EXACT quarter turn of a canvas whose `W + H` is odd
//! gives the right size but the wrong placement: 63×48 at 90° plans 48×63
//! with the matrix `[0, 1, −1, 0, 47.5, −0.5]` where `rotate90` is
//! `[0, 1, −1, 0, 48, 0]`, a half-pixel resample of a lossless permutation.
//! So a quarter turn is
//! DELEGATED: [`DrDocument::rotate_canvas_with_job`] answers `rotate90()`,
//! `rotate180()` or `rotate270()` VERBATIM — pixels, masks, channels,
//! offsets, canvas, `geometry_guides`, `geometry_origin` and the TRANSPOSED
//! PPI included. "Exact" is [`doc_transform::exact_form`] on the linear part
//! built from the NORMALIZED angle, i.e. within `EXACT_EPSILON` (1e-9, so
//! `|Δθ| ≲ 5.7e-8°`).
//!
//! The host delegates a second time, and for a different reason: a
//! quarter-classified angle runs the existing `applyingDocumentGeometry`
//! path, so text and shape descriptions COMPOSE and stay
//! editable, and the recorded Action step keeps its integer form.
//!
//! The DISCONTINUITY that follows is disclosed rather than smoothed: 90°
//! permutes the guides and transposes the PPI, while 89.999° shifts and drops
//! guides (R6) and leaves the PPI alone. Those are two different operations
//! and they are meant to look it.
//!
//! # R9 — a saved channel is resampled STRAIGHT into the new canvas
//!
//! A channel is canvas space, so it never grows an extent of its own: each
//! one is walked from `(0, 0, W, H)` into `(0, 0, W′, H′)` through the
//! plan's inverse with `outside = 0`, which is `transform_channels` and the
//! canvas growth in one walk with no intermediate plane. That is why no
//! coverage that lands inside the new canvas can be lost even when `W′ < W`.
//!
//! # R10 — a retained source rides (since 12F-2)
//!
//! 12F-1b3 refused a document holding a retained source here, because
//! `doc_align::straighten_layers` would rerender a bound master under a
//! canvas that is simultaneously changing size. 12F-2 (`07d9153`) removed
//! that refusal: retained sources ride whole-document geometry, including a
//! filtered `rotate_canvas`, and `core/tests/retained_geometry.rs` covers it.
//!
//! # N2 — what rides unchanged
//!
//! Layer ids, locks and links (the op is lock-EXEMPT, exactly as
//! `straighten_layers` is: a Lock-All layer rotates with the picture),
//! labels, blend modes, opacity, clip flags, layer styles including
//! knockout (the scale factor is `sqrt(|det|) = 1`, so `scaled_style` hands
//! back the same `Arc`), the evaluation policy and the metadata. There is no
//! `.rz` change. Layer comps ride with their POSITIONS REMAPPED: each bound
//! raster entry's recorded offset goes where this rotation would have put
//! the layer sitting there (`comps_geometry`, through
//! `straighten_layers`), exactly as every other whole-canvas op remaps it.
//!
//! # What this module deliberately does NOT do
//!
//! No pixel arithmetic of its own. The layers go through
//! `doc_align::straighten_layers`, the channels through
//! `doc_transform::resample_mask_with_context`, the chrome through
//! `doc_guide::padded_*`, the saved-channel budget through
//! `doc_channel::channels_fit_at`, asked by `geometry_edit_plan`, and
//! the structure gate through `doc_group::validated`. Every one of those
//! is a shipped byte contract and each is the specification for its part.
//!
//! # The job path
//!
//! The host reaches this op only through `geometry_edit`'s byte
//! `rotate_canvas` request, on the bounded worker:
//! [`DrDocument::rotate_canvas_with_job`] polls and reports per layer (through
//! `straighten_layers_with_job`) and then per channel.
//!
//! This module doc is the R1–R12 contract. `rotation_kind` normalizes into (−180, 180]
//! (`ViewRotation.normalize`'s formula, written once more here) and asks
//! `doc_transform::exact_form` on the linear part `[cos, sin, −sin, cos]` built from the NORMALIZED
//! angle; `rotated_canvas` is R2's box — the extent of the four canvas corners rotated about
//! `(W/2, H/2)`, snapped and rounded UP, `ceil(bw) × ceil(bh)` — with the origin
//! `(⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)`. It performs NO pixel arithmetic of its
//! own — layers and every mask through `doc_align::straighten_layers`, each canvas-space CHANNEL
//! straight from the old canvas into `(0, 0, W′, H′)` through
//! `doc_transform::resample_mask_with_context` (so nothing inside the new canvas is lost when an
//! axis shrinks), chrome through `doc_guide::padded_*`, the saved-channel budget through
//! `doc_channel::channels_fit_at` in `geometry_edit_plan`, ahead of admission, the gate
//! through `doc_group::validated`. It is lock-EXEMPT, the style factor is exactly 1, the PPI is NOT
//! transposed on a filtered angle, a retained source rides, rerendered from its immutable master
//! (R10, since 12F-2; `retained_geometry.rs`) and there is no `.rz` change. The op is reached only
//! through `dr_doc_geometry_edit`'s byte `rotate_canvas` request, which answers NULL with NO
//! message for the identity and NULL with a sentence for a refusal; under a job
//! `rotate_canvas_with_job` polls per layer (through `doc_align::straighten_layers_with_job`) and
//! then per channel.

use std::sync::Arc;

use crate::doc::{DrDocument, Geometry, LayerKind, MAX_PIXELS};
use crate::doc_channel::Channel;
use crate::doc_geometry_set::filter_type;
use crate::doc_group::validated;
use crate::doc_guide::{padded_guides, padded_origin};
use crate::doc_transform::{
    exact_form, resample_mask_with_context, snap_to_integer, Affine, Exact, SourceMap,
};
use crate::job_control::JobContext;
use crate::precision_sampling::GeometryFilter;
use crate::reference_point::{rotate_canvas_map, ReferenceOp};

/// Whether an exact whole-canvas result differs in any state the operation
/// can move. Comparisons use stored samples, including hidden RGB, rather
/// than a projection; retained-source binding validity is part of the state.
pub(crate) fn exact_geometry_changed(before: &DrDocument, after: &DrDocument) -> bool {
    if before.width != after.width
        || before.height != after.height
        || before.resolution != after.resolution
        || before.guides != after.guides
        || before.ruler_origin != after.ruler_origin
        || before.color_samplers != after.color_samplers
        || before.paths.entries != after.paths.entries
        // The run's unread path records (`tiff_paths::canvas_moved`).
        || before.metadata != after.metadata
        || before.layers.len() != after.layers.len()
        || before.channels.len() != after.channels.len()
        || before.comps != after.comps
    {
        return true;
    }
    for (left, right) in before.layers.iter().zip(&after.layers) {
        if left.offset != right.offset
            || left.pixels.dimensions() != right.pixels.dimensions()
            || !left.pixels.same_bytes(&right.pixels)
            || left.mask_enabled != right.mask_enabled
            || left.mask_properties != right.mask_properties
            || left.mask.is_some() != right.mask.is_some()
            || left.effective_source(&before.profile).is_some()
                != right.effective_source(&after.profile).is_some()
        {
            return true;
        }
        if let (Some(left), Some(right)) = (&left.mask, &right.mask) {
            if left.dimensions() != right.dimensions() || left.as_raw() != right.as_raw() {
                return true;
            }
        }
    }
    before
        .channels
        .iter()
        .zip(&after.channels)
        .any(|(left, right)| {
            left.data.dimensions() != right.data.dimensions()
                || left.data.as_raw() != right.data.as_raw()
        })
}

/// What an angle MEANS to this op, once normalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationKind {
    /// A whole number of full turns: nothing moves, and the op is a
    /// successful no-op at every depth.
    Identity,
    /// An exact quarter turn clockwise — `rotate90()` verbatim.
    Rotate90,
    /// An exact half turn — `rotate180()` verbatim.
    Rotate180,
    /// An exact quarter turn counter-clockwise — `rotate270()` verbatim.
    Rotate270,
    /// Everything else: the canvas expands and every plane is resampled.
    Filtered,
}

/// The plan for a FILTERED rotation of a `W × H` canvas: the matrix that
/// takes old-canvas coordinates to new-canvas coordinates, and the new
/// canvas's rectangle expressed in OLD-canvas coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotatedCanvas {
    /// The canvas→canvas affine in `[a, b, c, d, tx, ty]` order: a rotation
    /// about `(W/2, H/2)` composed with the translation by R2's integer
    /// origin, where the old canvas's top-left lands.
    pub matrix: [f64; 6],
    /// Where the new canvas's left edge sits in old-canvas coordinates.
    /// NEGATIVE when the canvas grew on that axis, positive when it shrank.
    pub left: i32,
    /// Where the new canvas's top edge sits in old-canvas coordinates.
    pub top: i32,
    /// The new canvas width.
    pub width: u32,
    /// The new canvas height.
    pub height: u32,
}

impl RotatedCanvas {
    /// Where the OLD canvas's top-left corner lands in the NEW canvas — the
    /// `origin` `doc_guide::padded_guides` and `padded_origin` take, and
    /// R2's `(⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)`.
    pub fn origin(&self) -> (i32, i32) {
        (-self.left, -self.top)
    }
}

/// R1's normalization: `degrees` into `(−180, 180]`, with `−0` folded onto
/// `0`. `None` when the angle is not finite.
fn normalize(degrees: f64) -> Option<f64> {
    if !degrees.is_finite() {
        return None;
    }
    let mut value = degrees % 360.0;
    if value <= -180.0 {
        value += 360.0;
    }
    if value > 180.0 {
        value -= 360.0;
    }
    // −0.0 is not 0.0 to a JSON reader, and a reported "-0" would be the one
    // number a caller could not compare with 0.
    Some(if value == 0.0 { 0.0 } else { value })
}

/// The LINEAR part of a rotation by `degrees`, as a translation-free affine.
fn linear(degrees: f64) -> Affine {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    Affine::from_array([cos, sin, -sin, cos, 0.0, 0.0])
}

/// What `degrees` means, plus the normalized angle itself. `None` only when
/// the angle is not finite — every finite angle classifies.
///
/// The classification is [`exact_form`] on the linear part built from the
/// NORMALIZED angle, which is R3's rule and the reason 450° is a quarter turn
/// and 90.001° is not. A rotation's linear part can never match a flip, so
/// only the four exact forms below are reachable.
pub fn rotation_kind(degrees: f64) -> Option<(RotationKind, f64)> {
    let normalized = normalize(degrees)?;
    let kind = match exact_form(&linear(normalized)) {
        Some(Exact::Translate) => RotationKind::Identity,
        Some(Exact::Geom(Geometry::Rotate90)) => RotationKind::Rotate90,
        Some(Exact::Geom(Geometry::Rotate180)) => RotationKind::Rotate180,
        Some(Exact::Geom(Geometry::Rotate270)) => RotationKind::Rotate270,
        _ => RotationKind::Filtered,
    };
    Some((kind, normalized))
}

/// R2's expanded canvas for ANY finite angle — the classification is NOT
/// consulted, so a caller can ask what an exact quarter turn's box would have
/// been (and a test does, which is how R3's half-pixel hazard is measured).
///
/// The size is `ceil(bw) × ceil(bh)`: the extent of the four
/// corners rotated about `(W/2, H/2)`, each snapped by `snap_to_integer` and
/// the extent snapped once more, so a composition's residue cannot add a
/// pixel. The old canvas's top-left lands at the FLOOR of the centred
/// placement, `(⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)` (R2).
///
/// `None` when the angle or a corner is not finite, when either side or
/// either extent is zero, when `W′·H′` is past `MAX_PIXELS`, or when `left`
/// or `top` is outside the `i32` offset range.
pub fn rotated_canvas(width: u32, height: u32, degrees: f64) -> Option<RotatedCanvas> {
    if width == 0 || height == 0 {
        return None;
    }
    let normalized = normalize(degrees)?;
    let r = linear(normalized);
    let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    // T(cx, cy) . R(θ) . T(−cx, −cy): the rotation about the canvas centre.
    let centred = Affine {
        tx: cx - r.a * cx - r.c * cy,
        ty: cy - r.b * cx - r.d * cy,
        ..r
    };
    let (w, h) = (f64::from(width), f64::from(height));
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in [
        centred.apply(0.0, 0.0),
        centred.apply(w, 0.0),
        centred.apply(0.0, h),
        centred.apply(w, h),
    ] {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x, y) = (snap_to_integer(x), snap_to_integer(y));
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    let extent_w = snap_to_integer(max.0 - min.0).ceil();
    let extent_h = snap_to_integer(max.1 - min.1).ceil();
    // Both extents are at most the canvas diagonal, so the casts are exact
    // once the zero and MAX_PIXELS checks pass.
    if !(extent_w >= 1.0 && extent_h >= 1.0) {
        return None;
    }
    let (new_w, new_h) = (extent_w as u64, extent_h as u64);
    if new_w.checked_mul(new_h)? > MAX_PIXELS {
        return None;
    }
    let origin_x = (new_w as i64 - i64::from(width)).div_euclid(2);
    let origin_y = (new_h as i64 - i64::from(height)).div_euclid(2);
    let left = i32::try_from(-origin_x).ok()?;
    let top = i32::try_from(-origin_y).ok()?;
    // The same rotation with the old canvas's top-left placed at R2's integer origin `o`.
    let matrix = [
        centred.a,
        centred.b,
        centred.c,
        centred.d,
        centred.tx - f64::from(left),
        centred.ty - f64::from(top),
    ];
    Some(RotatedCanvas {
        matrix,
        left,
        top,
        width: new_w as u32,
        height: new_h as u32,
    })
}

/// R9's channel walk, the ONE copy: each canvas-space channel resampled
/// straight from the old canvas into `(0, 0, dest.0, dest.1)` through
/// `inverse` (new canvas → old canvas) with `outside = 0`. Channel `c`
/// reports stage 6 at `at(c)` before its resample in `at(c) … at(c + 1)`.
/// `rotate_canvas_with_job` and the byte Straighten arm's extended window
/// (`doc_crop_extend`'s `extended_straighten`) are its callers.
pub(crate) fn channels_into(
    channels: &[Channel],
    inverse: &Affine,
    dest: (u32, u32),
    filter: GeometryFilter,
    job: JobContext<'_>,
    at: impl Fn(f64) -> f64,
) -> Result<Vec<Channel>, String> {
    let sampler = filter.sampler();
    let rect = (0, 0, dest.0, dest.1);
    let map = SourceMap::new(inverse, (0, 0), (0, 0));
    let mut out = Vec::with_capacity(channels.len());
    for (c, channel) in channels.iter().enumerate() {
        let c = c as f64;
        job.report(at(c), 6)?;
        out.push(Channel {
            data: Arc::new(resample_mask_with_context(
                &channel.data,
                rect,
                &map,
                sampler,
                job.range(at(c), at(c + 1.0)),
            )?),
            ..channel.clone()
        });
    }
    Ok(out)
}

impl DrDocument {
    /// Rotates the WHOLE document by `degrees` (positive is clockwise),
    /// expanding the canvas to hold the turned picture.
    ///
    /// `Ok(None)` means IDENTITY and nothing else: a whole number of full
    /// turns moves no sample, and every other outcome is either a new
    /// document or a named refusal. An exact quarter turn is
    /// `rotate90()` / `rotate180()` / `rotate270()` verbatim (R3), `filter`
    /// unread.
    ///
    /// For a FILTERED angle: every layer and EVERY mask rides
    /// `doc_align::straighten_layers` under the plan's matrix, every saved
    /// channel is resampled straight from the old canvas into the new one
    /// (R9), the guides and the ruler origin ride `doc_guide::padded_*` by
    /// the plan's origin, and the PPI is NOT transposed — 89.999° is not a
    /// quarter turn and must not pretend to be one.
    ///
    /// Refusals, each by its own sentence: a non-finite angle, a rotated
    /// canvas past the supported size, and a layer whose rotated extent
    /// cannot be built. The saved-channel budget is `geometry_edit_plan`'s
    /// refusal, ahead of admission; this method's only caller plans
    /// first.
    ///
    /// This is the byte `rotate_canvas` request of `geometry_edit`, under
    /// `job`: with L layers and C channels and `s = L/(L+C)`, the layers run
    /// `straighten_layers_with_job` in `0 … s` and channel c reports stage 6
    /// at `s + (1−s)·c/C` before its resample in its own slice. Every
    /// refusal keeps its sentence; the cancel is the only other `Err`.
    pub(crate) fn rotate_canvas_with_job(
        &self,
        degrees: f64,
        filter: GeometryFilter,
        job: JobContext<'_>,
        floor: u64,
    ) -> Result<Option<Self>, String> {
        let (kind, normalized) = rotation_kind(degrees).ok_or("rotation degrees must be finite")?;
        match kind {
            RotationKind::Identity => return Ok(None),
            RotationKind::Rotate90 => return Ok(self.rotate90()),
            RotationKind::Rotate180 => return Ok(self.rotate180()),
            RotationKind::Rotate270 => return Ok(self.rotate270()),
            RotationKind::Filtered => {}
        }
        let plan = rotated_canvas(self.width, self.height, normalized)
            .ok_or("the rotated canvas would exceed the supported canvas size")?;
        let m = Affine::from_array(plan.matrix);
        // A rotation is never singular, so this is a backstop rather than a
        // decision — and it is the same `inverse()` every resampler takes. It
        // answers the EXTENT sentence, like the `validated` arm below: a
        // matrix that cannot be inverted is one no layer's rotated extent can
        // be built from, and the canvas size was already admitted above.
        let inverse = m.inverse().ok_or(
            "a layer's rotated extent is empty, outside the i32 offset range or past the \
             pixel limit",
        )?;

        let members = self.layers.len() + self.channels.len();
        let s = if members == 0 {
            1.0
        } else {
            self.layers.len() as f64 / members as f64
        };
        let mut doc = match self.straighten_layers_with_job(
            m,
            filter_type(filter),
            job.range(0.0, s),
            floor,
        )? {
            Some(straightened) => straightened,
            // `straighten_layers` answers `None` both for a refused extent
            // and for a document with NOTHING to transform — a stack of
            // empty groups with no mask anywhere. The second is not a
            // refusal: those entries ride as they are, exactly as they do
            // under `canvas_resize`.
            None if !self
                .layers
                .iter()
                .any(|l| l.kind == LayerKind::Raster || l.mask.is_some()) =>
            {
                self.clone()
            }
            None => {
                return Err("a layer's rotated extent is empty, outside the i32 offset \
                            range or past the pixel limit"
                    .into())
            }
        };

        // R9: each canvas-space channel walked straight into the new canvas.
        let n = self.channels.len() as f64;
        let channels = channels_into(
            &self.channels,
            &inverse,
            (plan.width, plan.height),
            filter,
            job,
            |c: f64| s + (1.0 - s) * c / n,
        )?;

        let origin = plan.origin();
        doc.width = plan.width;
        doc.height = plan.height;
        doc.channels = channels;
        doc.guides = padded_guides(&self.guides, plan.width, plan.height, origin);
        doc.ruler_origin = padded_origin(self.ruler_origin, plan.width, plan.height, origin);
        // The samplers rotate WITH the content (`doc_sampler`), where a guide
        // — a line — can only shift.
        doc.color_samplers = crate::doc_sampler::matrix_samplers(
            &self.color_samplers,
            plan.matrix,
            (0, 0, plan.width, plan.height),
        );
        doc.paths.set_entries(crate::doc_path::matrix_paths(
            &self.paths.entries,
            plan.matrix,
            (0, 0),
        ));
        doc.metadata = crate::tiff_paths::canvas_moved(&self.metadata);
        // The PPI is untouched on purpose: only an EXACT quarter turn swaps
        // the axes, and this branch is by definition not one.
        validated(doc)
            .map(|doc| {
                let map = rotate_canvas_map(plan.matrix, (self.width, self.height));
                self.following_references(doc, ReferenceOp::Rotate, map)
            })
            .map(Some)
            .ok_or_else(|| {
                "a layer's rotated extent is empty, outside the i32 offset range or past the \
             pixel limit"
                    .to_string()
            })
    }
}
