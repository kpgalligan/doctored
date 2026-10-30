//! The byte Content-Aware Fill bridge: the metadata admission and the exact edit, the only byte fill entry points.
use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use std::{ffi::c_char, ptr};

/// Exact byte fill scratch bound, excluding retained inputs. Zero on refusal.
/// # Safety
/// doc NULL/live; request NULL or terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_content_aware_fill_scratch(
    doc: *const DrDocument,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning content-aware fill",
            0,
            || {
                let doc = required_handle(doc)?;
                let edit = crate::doc_content_aware::request(borrow_cstr_bounded(
                    request,
                    "request",
                    64 * 1024,
                )?)?;
                doc.content_aware_fill_scratch(&edit)
            },
            |n| n,
        )
    }
}
/// Exact byte candidate using strict content_aware_fill JSON and canvas coverage.
/// NULL result without error is an exact no-op. Control may be NULL; supplied
/// tokens are single-use. Cancellation refuses atomically and releases scratch.
/// # Safety
/// doc/control NULL/live; request NULL or terminated UTF-8; coverage NULL or
/// readable for doc canvas width*height bytes; err_out NULL/writable. Owners
/// outlive every call; free returned document/error using their matching frees.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_content_aware_fill_edit(
    doc: *const DrDocument,
    request: *const c_char,
    coverage: *const u8,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying content-aware fill",
            ptr::null_mut(),
            || {
                let doc = required_handle(doc)?;
                if coverage.is_null() {
                    return Err("content-aware selection is required".into());
                }
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(4 * 1024 * 1024)?;
                let edit = crate::doc_content_aware::request(borrow_cstr_bounded(
                    request,
                    "request",
                    64 * 1024,
                )?)?;
                let n = doc.width as usize * doc.height as usize;
                let coverage = std::slice::from_raw_parts(coverage, n);
                let result = doc.content_aware_fill_edit(&edit, coverage, job)?;
                job.report(1., 5)?;
                Ok(result)
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}
