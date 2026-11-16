//! The brush dynamics handle (`brush_dynamics`): a request parsed once, so a
//! stroke's walk of up to 50,000 stamps resolves each stamp without parsing
//! again. Immutable: every query takes `*const` and the handle may be shared
//! across threads.
use crate::brush_dynamics::{Dynamics, DAB_LEN, MAX_REQUEST_BYTES, SAMPLE_LEN};
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, handle_get};
use std::ffi::c_char;
use std::ptr;

/// The opaque handle `dr_brush_dynamics_create` returns.
pub struct DrBrushDynamics(Dynamics);

/// Parses a strict brush dynamics request (`brush_dynamics` module doc; at
/// most 4096 bytes). NULL with `err_out` set on a refusal; free the handle
/// with `dr_brush_dynamics_free`.
///
/// # Safety
/// `request` NULL or a NUL-terminated string; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dynamics_create(
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrBrushDynamics {
    if request.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "parsing brush dynamics",
            ptr::null_mut(),
            || {
                let text =
                    borrow_cstr_bounded(request, "brush dynamics request", MAX_REQUEST_BYTES)?;
                Dynamics::parse(text).map(DrBrushDynamics)
            },
            boxed,
        )
    }
}

/// Frees a dynamics handle; NULL is a no-op.
///
/// # Safety
/// `dynamics` NULL or an unfreed handle no call is using.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dynamics_free(dynamics: *mut DrBrushDynamics) {
    unsafe { free_handle(dynamics) }
}

/// The nominal stamp at `size` and the pen sample (NaN pressure or tilt: no
/// sample): writes the nominal diameter and deposit factor to `out2[0..2]`.
/// False, `out2` untouched, on NULL or a size that is not a finite number at
/// least 0.
///
/// # Safety
/// `dynamics` NULL or live; `out2` NULL or writable for two doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dynamics_nominal(
    dynamics: *const DrBrushDynamics,
    size: f64,
    pressure: f64,
    tilt_x: f64,
    tilt_y: f64,
    out2: *mut f64,
) -> bool {
    if out2.is_null() {
        return false;
    }
    unsafe {
        handle_get(dynamics, false, |d| {
            let Some((diameter, factor)) = d.0.nominal(size, pressure, (tilt_x, tilt_y)) else {
                return false;
            };
            *out2 = diameter;
            *out2.add(1) = factor;
            true
        })
    }
}

/// Stamp `index` resolved into its dabs: `sample` is 11 doubles (x, y, size,
/// pressure, tilt_x, tilt_y, dir_x, dir_y, rotation_degrees, heading_x,
/// heading_y) and each dab writes 6 to `out` (x, y, diameter, deposit factor,
/// angle_degrees, roundness). Returns the dabs written, 1 to 16; 0 with `out`
/// untouched on NULL, `sample_len` other than 11, `out_capacity` (in doubles)
/// below 6 × the scatter count, or a non-finite position or size.
///
/// # Safety
/// `dynamics` NULL or live; `sample` NULL or readable for `sample_len`
/// doubles; `out` NULL or writable for `out_capacity` doubles, not aliasing
/// `sample`.
#[no_mangle]
pub unsafe extern "C" fn dr_brush_dynamics_dabs(
    dynamics: *const DrBrushDynamics,
    index: u64,
    sample: *const f64,
    sample_len: usize,
    out: *mut f64,
    out_capacity: usize,
) -> usize {
    if sample.is_null() || out.is_null() || sample_len != SAMPLE_LEN {
        return 0;
    }
    let values: [f64; SAMPLE_LEN] = std::array::from_fn(|i| unsafe { *sample.add(i) });
    unsafe {
        handle_get(dynamics, 0, |d| {
            if out_capacity < DAB_LEN * d.0.max_dabs() {
                return 0;
            }
            let mut dabs = Vec::with_capacity(DAB_LEN * d.0.max_dabs());
            let Some(n) = d.0.dabs(index, &values, &mut dabs) else {
                return 0;
            };
            ptr::copy_nonoverlapping(dabs.as_ptr(), out, dabs.len());
            n
        })
    }
}
