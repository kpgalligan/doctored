//! Worker control and cancellable retouch entry points. The host owns thread
//! scheduling and retains immutable input handles/buffers until return.
//!
//! It reserves no parent scratch; the host owns child lifetime, cancellation forwarding and
//! progress aggregation. A claimed parent cannot be reused by another compound preparation or any
//! ordinary core operation, including after precancellation. Existing kernels and their
//! individual-token semantics are unchanged. `dr_job_control_set_working_limit` records the
//! host's whole working limit before start; a `reserve` over the scratch limit that is also at
//! least it says closing other documents will not help.

use crate::doc::DrDocument;
use crate::ffi_util::{boxed, fallible_op, produce_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::retouch_sampling::RetouchSampling;
use crate::spot_heal_type::SpotHealType;
use std::ffi::c_char;
use std::ptr;

/// Create a single-use thread-safe job control, NULL on allocation failure.
/// # Safety
/// The returned handle must be freed exactly once after all calls finish.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_create() -> *mut DrJobControl {
    produce_op(|| Some(DrJobControl::default()))
}

/// Free a control; NULL is a no-op. Never free during any worker/query call.
/// # Safety
/// `control` must be NULL or a live exclusively owned control handle.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_free(control: *mut DrJobControl) {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "freeing job control",
            (),
            || {
                if !control.is_null() {
                    drop(Box::from_raw(control));
                }
                Ok(())
            },
            |()| (),
        )
    }
}

/// Request cancellation; NULL is a no-op. Safe concurrent with a worker.
/// # Safety
/// `control` must be NULL or valid for this call.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_cancel(control: *const DrJobControl) {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "cancelling job",
            (),
            || {
                if let Some(control) = control.as_ref() {
                    control.cancel();
                }
                Ok(())
            },
            |()| (),
        )
    }
}

/// Set scratch budget before worker start; 0 means unlimited. False for NULL
/// or an already used token. Must not race worker start.
/// # Safety
/// `control` must be NULL or valid and not being started concurrently.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_set_scratch_limit(
    control: *const DrJobControl,
    bytes: u64,
) -> bool {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "setting job budget",
            false,
            || Ok(control.as_ref().is_some_and(|c| c.set_scratch_limit(bytes))),
            |value| value,
        )
    }
}

/// Set the host's whole working limit before worker start (0 = unknown).
/// A reservation over the scratch limit that is also at least this says
/// closing other documents will not help. False for NULL or an already used
/// token. Must not race worker start.
/// # Safety
/// `control` must be NULL or valid and not being started concurrently.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_set_working_limit(
    control: *const DrJobControl,
    bytes: u64,
) -> bool {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "setting job working limit",
            false,
            || Ok(control.as_ref().is_some_and(|c| c.set_working_limit(bytes))),
            |value| value,
        )
    }
}

/// Claim a single-use parent for host-composed sequential child operations.
/// The host aggregates progress and forwards cancellation; the parent reserves
/// no core scratch. Every child core operation still requires its own token.
/// False for NULL, cancellation or reuse; optional error is caller-owned.
/// # Safety
/// `control` is NULL or live through this call; `err_out` is NULL or writable.
/// Free an owned error with `dr_string_free`. Child lifetimes belong to the host.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_begin_compound(
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    if control.is_null() {
        return false;
    }
    unsafe {
        fallible_op(
            err_out,
            "claiming compound job",
            false,
            || {
                let _run = (&*control).begin()?;
                Ok(true)
            },
            |value| value,
        )
    }
}

/// Read the job's progress; returns 0.0 for NULL. Concurrent-query safe.
/// # Safety
/// `control` must be NULL or a live control for this call.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_progress(control: *const DrJobControl) -> f64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "querying job",
            0.0,
            || Ok(control.as_ref().map_or(0.0, DrJobControl::progress)),
            |value| value,
        )
    }
}

/// Read the job's stage; returns 0 for NULL. Concurrent-query safe.
/// # Safety
/// `control` must be NULL or a live control for this call.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_stage(control: *const DrJobControl) -> u32 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "querying job",
            0,
            || Ok(control.as_ref().map_or(0, DrJobControl::stage)),
            |value| value,
        )
    }
}

/// Read the job's cancelled; returns false for NULL. Concurrent-query safe.
/// # Safety
/// `control` must be NULL or a live control for this call.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_cancelled(control: *const DrJobControl) -> bool {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "querying job",
            false,
            || Ok(control.as_ref().is_some_and(DrJobControl::cancelled)),
            |value| value,
        )
    }
}

/// Read the job's scratch bytes; returns 0 for NULL. Concurrent-query safe.
/// # Safety
/// `control` must be NULL or a live control for this call.
#[no_mangle]
pub unsafe extern "C" fn dr_job_control_scratch_bytes(control: *const DrJobControl) -> u64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "querying job",
            0,
            || Ok(control.as_ref().map_or(0, DrJobControl::scratch_bytes)),
            |value| value,
        )
    }
}

/// Spot healing, the app's one spot-heal entry point: inpaints the
/// overlay's footprint from a ring of valid pixels around it, then
/// Poisson-blends the result in exactly as `dr_doc_heal_layer` does. Only
/// the overlay's ALPHA is read. `ring` is the sampling-ring width in px
/// (0 = automatic), `seed` makes the result reproducible, `spot_type` is the
/// Type (0 Content-Aware, 1 Create Texture, 2 Proximity Match), `sample` picks
/// the sampled layers (current, current-and-below or all) with
/// `ignore_adjustments` skipping adjustment layers. NULL with a message on a
/// cap or a starved sample region; NULL with no message when nothing would
/// change.
///
/// Control: a NULL `doc` or `src` answers NULL with no message before
/// `control` is read. A NULL `control` is then refused with "job control is
/// NULL". The token is claimed next — before the remaining shape refusals
/// (canvas-size mismatch, an unknown `spot_type` or `sample` code), which
/// therefore consume it — and is single-use: a consumed one is refused with "job control has
/// already been used". A cancel answers NULL with "operation cancelled",
/// never a partial document. Scratch is 0 on return, and a completed run
/// ends at 1.0 / stage 5.
///
/// # Safety
/// `doc` and `control` must be NULL or live for the complete call; the input
/// buffer must be NULL or valid for canvas width*height*4 bytes. `err_out`
/// must be NULL or writable; free errors with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_spot_heal_layer_job(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    strength: f32,
    ring: u32,
    seed: u64,
    spot_type: u32,
    sample: u32,
    ignore_adjustments: bool,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        if doc.is_null() || src.is_null() {
            return Ok(None);
        }
        let control = unsafe { control.as_ref() }.ok_or("job control is NULL")?;
        let _run = control.begin()?;
        let document = unsafe { &*doc };
        if w != document.width || h != document.height {
            return Ok(None);
        }
        let Some(len) = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
        else {
            return Ok(None);
        };
        let overlay = unsafe { std::slice::from_raw_parts(src, len) };
        let Some(kind) = SpotHealType::from_code(spot_type) else {
            return Ok(None);
        };
        let Some(sample) = RetouchSampling::from_code(sample) else {
            return Ok(None);
        };
        let result = document.spot_heal_layer_job(
            idx,
            overlay,
            w,
            h,
            strength,
            ring,
            seed,
            kind,
            sample,
            ignore_adjustments,
            JobContext::new(control),
        )?;
        control.checkpoint()?;
        control.report(1.0, 5);
        Ok(result)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while spot healing",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}
