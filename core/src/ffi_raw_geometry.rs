//! Additive RAW source-coordinate geometry exports. Existing RAW ABI is unchanged.
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::raw_local_mask::RawLocalMask;
use std::ffi::{c_char, CString};
use std::ptr;

const REQUEST_CAP: usize = 4096;

/// # Safety
/// mask NULL/live; request NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_remap_plan(
    mask: *const RawLocalMask,
    request: *const c_char,
    channel: u32,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning RAW local remap",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "RAW source map", REQUEST_CAP)?;
                CString::new(
                    required_handle(mask)?
                        .remap_plan(request, channel, limit, other)?
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// # Safety
/// mask/control NULL/live; request NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_remap(
    mask: *const RawLocalMask,
    request: *const c_char,
    channel: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut RawLocalMask {
    unsafe {
        fallible_op(
            err_out,
            "remapping RAW local coverage",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "RAW source map", REQUEST_CAP)?;
                required_handle(mask)?.remap(request, channel, limit, other, control.as_ref())
            },
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// A new mask of `width` x `height` holding `mask`'s coverage in normalised coordinates
/// (bilinear, edge-normalised). NULL without an error when the size is unchanged; an error for
/// a NULL mask, a zero side, more than 64 megapixels, a refused admission or a cancel.
///
/// # Safety
/// mask/control NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_resize(
    mask: *const RawLocalMask,
    width: u32,
    height: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut RawLocalMask {
    unsafe {
        fallible_op(
            err_out,
            "resizing RAW local coverage",
            ptr::null_mut(),
            || required_handle(mask)?.resized(width, height, limit, other, control.as_ref()),
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// # Safety
/// source NULL/live; request NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_source_geometry_plan(
    source: *const PrecisionDocument,
    request: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning RAW source geometry",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "RAW source map", REQUEST_CAP)?;
                CString::new(
                    required_handle(source)?
                        .raw_source_geometry_plan(request, limit, other)?
                        .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// # Safety
/// source/control NULL/live; request NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_source_geometry(
    source: *const PrecisionDocument,
    request: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "mapping RAW source geometry",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "RAW source map", REQUEST_CAP)?;
                required_handle(source)?.raw_source_geometry(
                    request,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}
