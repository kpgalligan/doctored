//! Generative Expand's bridge (M3b phase 2): the coverage op at 8 bits
//! and on 16-bit RGB, 32-bit float RGB or 8-bit CMYK, over `doc_gen_expand`. The host then
//! runs Generative Fill's frame, input and apply (`ffi_gen_fill`) over the
//! coverage. The buffer length is recomputed from the core's own canvas
//! before a slice is formed.

use std::ffi::c_char;

use crate::doc::DrDocument;
use crate::ffi_util::{fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// A writable slice of exactly `expected` u16 samples, else the expand
/// buffer refusal (`ffi_gen_fill::exact_mut` is u8-only and carries the
/// fill sentence).
///
/// # Safety
/// `data` must be NULL or writable for `count` u16s.
unsafe fn coverage_mut<'a>(
    data: *mut u16,
    count: usize,
    expected: u64,
) -> Result<&'a mut [u16], String> {
    if data.is_null() || count as u64 != expected || count == 0 {
        return Err(crate::doc_gen_expand::BAD_BUFFER.into());
    }
    // The length is the caller's, checked against the core's own figure.
    Ok(unsafe { std::slice::from_raw_parts_mut(data, count) })
}

/// Writes Generative Expand's coverage of a byte document into
/// `coverage_out` (`coverage_count == width·height` u16s): `65535 −` each
/// pixel's full-composite alpha (u16; a byte alpha × 257), so the area the
/// picture leaves transparent is the hole. `layer` is the anchor (range
/// checked only). Admits the plate against `limit` beside `other`. False with
/// `err_out` for a NULL document or buffer, a mis-sized buffer, an
/// out-of-range layer, a refused plan, a cancel and a picture with no
/// transparent pixel.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `coverage_out` NULL or writable
/// for `coverage_count` u16s; `control` NULL or live; `err_out` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gen_expand_coverage(
    doc: *const DrDocument,
    layer: usize,
    coverage_out: *mut u16,
    coverage_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "reading the generative expand coverage",
            false,
            || {
                let document = required_handle(doc)?;
                let n = u64::from(document.width) * u64::from(document.height);
                let coverage = coverage_mut(coverage_out, coverage_count, n)?;
                document.gen_expand_coverage(layer, coverage, limit, other, control.as_ref())?;
                Ok(true)
            },
            |written| written,
        )
    }
}

/// `dr_doc_gen_expand_coverage` for a 16-bit RGB, 32-bit float RGB or 8-bit
/// CMYK document (the alpha read from its own plate, a float alpha clamped to
/// 0…1); a 16-bit CMYK document refuses by name.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; the rest as
/// `dr_doc_gen_expand_coverage`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_gen_expand_coverage(
    doc: *const PrecisionDocument,
    layer: usize,
    coverage_out: *mut u16,
    coverage_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "reading the generative expand coverage",
            false,
            || {
                let document = required_handle(doc)?;
                let (w, h) = document.dimensions();
                let n = u64::from(w) * u64::from(h);
                let coverage = coverage_mut(coverage_out, coverage_count, n)?;
                document.gen_expand_coverage(layer, coverage, limit, other, control.as_ref())?;
                Ok(true)
            },
            |written| written,
        )
    }
}
