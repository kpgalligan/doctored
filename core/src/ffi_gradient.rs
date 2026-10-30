//! The stops gradient across the C boundary: `dr_doc_gradient_stops`,
//! and the midpoint remap the host's gradient editor draws through:
//! `dr_gradient_midpoint_fraction`. The two-colour `dr_doc_gradient` stays in
//! `ffi_doc`.

use std::os::raw::c_int;
use std::panic::catch_unwind;
use std::ptr;

use crate::blend::BlendMode;
use crate::doc::DrDocument;
use crate::ffi_util::{doc_op, mask_slice};
use crate::gradient_field::{midpoint_fraction, FieldStop, FieldStyle, MIDPOINT_MAX, MIDPOINT_MIN};
use crate::style_json::GRADIENT_STOPS;

/// Floats per stop across the boundary: `[position, r, g, b, a, midpoint,
/// opacity_midpoint]`.
const STOP_FLOATS: usize = 7;

/// Paints a stops gradient onto layer `idx` along (x0, y0) → (x1, y1) through
/// blend mode `mode` (a `DrBlendMode`; Normal is plain source-over):
/// `stops` is `stop_count` × 7 floats `[position, r, g, b, a, midpoint,
/// opacity_midpoint]`, straight and in 0..1, 2..32 entries in non-decreasing
/// position, each midpoint in 0.05..=0.95 (0.5 the default; the
/// segment toward the next stop, `gradient_field::midpoint_fraction`); `kind`
/// is a `DrGradientKind` (0 linear, 1 radial, 2 angle, 3 reflected, 4
/// diamond); `smoothness` is the Smoothness option as a fraction (1 = 100 %,
/// the Classic ramp; 0 the linear one; between them the two blended,
/// `gradient_field::smooth_color`; Diamond ignores it); `mask` is canvas-sized
/// selection coverage or NULL. NULL for a NULL document or `stops`, a stop
/// count outside 2..32, a stop out of range or order, a midpoint outside
/// 0.05..=0.95, a smoothness not finite in 0..=1, an unknown kind, a mode
/// `dr_doc_fill_selection` refuses (unknown, or the group-only Pass Through),
/// equal or non-finite endpoints, a group or
/// out-of-range index, a pixel lock, or a gradient that changes nothing.
///
/// # Safety
/// `doc` must be NULL or a valid live `DrDocument`; `stops` must be NULL or
/// readable for `stop_count * 7` floats; `mask` must be NULL or valid for
/// canvas width*height bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_gradient_stops(
    doc: *const DrDocument,
    idx: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    stops: *const f32,
    stop_count: usize,
    kind: u32,
    mode: c_int,
    smoothness: f32,
    mask: *const u8,
) -> *mut DrDocument {
    if stops.is_null() || !GRADIENT_STOPS.contains(&stop_count) {
        return ptr::null_mut();
    }
    let Some(style) = FieldStyle::from_c(kind) else {
        return ptr::null_mut();
    };
    let Some(mode) = BlendMode::from_c(mode) else {
        return ptr::null_mut();
    };
    unsafe {
        doc_op(doc, |d| {
            let values = std::slice::from_raw_parts(stops, stop_count * STOP_FLOATS);
            let stops: Vec<FieldStop> = values
                .chunks_exact(STOP_FLOATS)
                .map(|s| FieldStop {
                    position: s[0],
                    color: [s[1], s[2], s[3], s[4]],
                    midpoint: s[5],
                    opacity_midpoint: s[6],
                })
                .collect();
            let mask = mask_slice(mask, d.width as usize * d.height as usize);
            d.gradient_stops(
                idx,
                (x0, y0),
                (x1, y1),
                &stops,
                style,
                smoothness,
                mode,
                mask,
            )
        })
    }
}

/// The segment fraction `f` remapped through a stop's `midpoint`:
/// `gradient_field::midpoint_fraction`, the ONE remap, which the host's
/// gradient editor strip draws through rather than keeping a copy. `f` is
/// clamped to 0..1 (NaN 0) and `midpoint` to 0.05..0.95 (NaN 0.5); at 0.5 it
/// answers `f` exactly. Pointer-free, so it takes and refuses nothing.
#[no_mangle]
pub extern "C" fn dr_gradient_midpoint_fraction(f: f32, midpoint: f32) -> f32 {
    // Pure arithmetic that cannot panic; the guard is the family's uniform
    // shape, not a defence against a known unwind.
    catch_unwind(|| {
        let f = if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) };
        let m = if midpoint.is_nan() {
            0.5
        } else {
            midpoint.clamp(MIDPOINT_MIN, MIDPOINT_MAX)
        };
        midpoint_fraction(f, m)
    })
    .unwrap_or(0.0)
}
