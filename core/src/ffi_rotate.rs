//! FFI for DESTRUCTIVE whole-canvas rotation by any finite angle
//! (`doc_rotate`, Phase 12F-1b3): the PLAN export that makes the core the
//! only place an angle is classified or a rotated canvas is sized. The op
//! itself is `dr_doc_geometry_edit`'s byte `rotate_canvas` request,
//! which runs on the bounded worker under a job.
//!
//! The plan export takes no handle at all. That is deliberate: the host asks
//! it from a sheet that has not committed anything, from the MCP handler
//! before it opens an undo group, and from `ActionGeometry.canvasAfter`,
//! which has only a recorded canvas SIZE and no document. A rule the host
//! could restate is a rule the two sides can disagree about.
//!
//! `dr_canvas_rotation_plan` takes NO handle, because the sheet, the MCP handler and
//! `ActionGeometry.canvasAfter` each need the rule with no document in hand.

use std::ptr;

use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};

/// `dr_canvas_rotation_plan`'s answer for an angle that is not finite.
pub const DR_ROTATION_REFUSED: i32 = 0;
/// A whole number of full turns: a successful no-op.
pub const DR_ROTATION_IDENTITY: i32 = 1;
/// An exact quarter turn clockwise.
pub const DR_ROTATION_90: i32 = 2;
/// An exact half turn.
pub const DR_ROTATION_180: i32 = 3;
/// An exact quarter turn counter-clockwise.
pub const DR_ROTATION_270: i32 = 4;
/// Everything else: the canvas expands and every plane resamples.
pub const DR_ROTATION_FILTERED: i32 = 5;

/// Classifies `degrees` for a `width` x `height` canvas and reports the
/// rectangle the NEW canvas occupies in OLD-canvas coordinates.
///
/// `out_degrees` receives the NORMALIZED angle, in `(-180, 180]`, with `-0`
/// folded onto `0` (untouched when the angle is not finite). `out_rect`
/// receives `left, top, width, height`: `(0, 0, W, H)` for identity and 180,
/// `(0, 0, H, W)` for the two quarter turns, R2's `ceil(bw) x ceil(bh)` box
/// whose `left, top` are minus the FLOOR origin `(floor((W'-W)/2),
/// floor((H'-H)/2))` when filtered, and ALL ZEROS when that box is refused
/// (not finite, a zero side, an
/// `i32`-range overflow or past the 100 MP pixel budget). Either out pointer
/// may be NULL.
///
/// # Safety
/// `out_degrees` must be NULL or a valid pointer to a writable `f64`;
/// `out_rect` must be NULL or a valid pointer to four writable `i32`s.
#[no_mangle]
pub unsafe extern "C" fn dr_canvas_rotation_plan(
    width: u32,
    height: u32,
    degrees: f64,
    out_degrees: *mut f64,
    out_rect: *mut i32,
) -> i32 {
    let mut rect = [0i32; 4];
    let mut normalized = 0.0f64;
    let code = match rotation_kind(degrees) {
        None => DR_ROTATION_REFUSED,
        Some((kind, angle)) => {
            normalized = angle;
            let (w, h) = (
                i32::try_from(width).unwrap_or(0),
                i32::try_from(height).unwrap_or(0),
            );
            match kind {
                RotationKind::Identity | RotationKind::Rotate180 => {
                    rect = [0, 0, w, h];
                }
                RotationKind::Rotate90 | RotationKind::Rotate270 => {
                    rect = [0, 0, h, w];
                }
                RotationKind::Filtered => {
                    if let Some(plan) = rotated_canvas(width, height, angle) {
                        rect = [
                            plan.left,
                            plan.top,
                            i32::try_from(plan.width).unwrap_or(0),
                            i32::try_from(plan.height).unwrap_or(0),
                        ];
                    }
                }
            }
            match kind {
                RotationKind::Identity => DR_ROTATION_IDENTITY,
                RotationKind::Rotate90 => DR_ROTATION_90,
                RotationKind::Rotate180 => DR_ROTATION_180,
                RotationKind::Rotate270 => DR_ROTATION_270,
                RotationKind::Filtered => DR_ROTATION_FILTERED,
            }
        }
    };
    if !out_degrees.is_null() && code != DR_ROTATION_REFUSED {
        unsafe { *out_degrees = normalized };
    }
    if !out_rect.is_null() {
        unsafe { ptr::copy_nonoverlapping(rect.as_ptr(), out_rect, 4) };
    }
    code
}
