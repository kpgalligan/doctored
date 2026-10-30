//! C FFI for `heif_container`: [`dr_heif_primary_color_facts`], which
//! reads the primary item's colour facts from an ISO-BMFF/HEIF file's own
//! boxes without decoding it, and [`dr_heif_premultiplied_reference`],
//! which locates the `prem` reference those facts report.

use std::ffi::c_char;

use crate::ffi_util::{fallible_op, set_err};
use crate::heif_container::{premultiplied_reference, primary_color_facts};

/// Primary-item colour facts read from the `length` bytes at `bytes` (an
/// ISO-BMFF/HEIF file), no decode. true: `*matrix_coefficients_out` is the
/// nclx `matrix_coefficients` (0..65535) of the first `colr`/`nclx` property
/// `ipma` associates with `pitm`'s item, or -1 if none, and
/// `*premultiplied_alpha_out` is whether an `iref` `prem` reference has that
/// item as its `from_item_ID` (pointing at the alpha item). false: the file
/// is malformed or not ISO-BMFF, `err_out` is set and the outputs are
/// untouched. NULL `bytes` or a NULL output returns false. The borrowed bytes
/// live for the call; never panics.
///
/// # Safety
/// `bytes` must be NULL or readable for `length` bytes; each output must be
/// NULL or a valid writable pointer; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_heif_primary_color_facts(
    bytes: *const u8,
    length: usize,
    matrix_coefficients_out: *mut i32,
    premultiplied_alpha_out: *mut bool,
    err_out: *mut *mut c_char,
) -> bool {
    if bytes.is_null() || matrix_coefficients_out.is_null() || premultiplied_alpha_out.is_null() {
        unsafe {
            set_err(
                err_out,
                "heif primary color facts need bytes and both outputs",
            )
        };
        return false;
    }
    let file = unsafe { std::slice::from_raw_parts(bytes, length) };
    unsafe {
        fallible_op(
            err_out,
            "heif primary color facts",
            false,
            || primary_color_facts(file),
            |facts| {
                *matrix_coefficients_out = facts.matrix_coefficients.map_or(-1, i32::from);
                *premultiplied_alpha_out = facts.premultiplied_alpha;
                true
            },
        )
    }
}

/// Where the primary item's `iref` `prem` reference names it, in the `length`
/// bytes at `bytes` (an ISO-BMFF/HEIF file), no decode. true:
/// `*offset_out` is the byte offset of that child's `from_item_ID` field and
/// `*width_out` its width (2 for an `iref` of version 0, else 4); when the
/// file has no `prem` child whose `from_item_ID` is the primary item,
/// `*width_out` is 0 and `*offset_out` is 0. Only the first such child is
/// located. false: the file is malformed or not ISO-BMFF, `err_out` is set
/// and the outputs are untouched. NULL `bytes` or a NULL output returns
/// false. The borrowed bytes live for the call; never panics.
///
/// # Safety
/// `bytes` must be NULL or readable for `length` bytes; each output must be
/// NULL or a valid writable pointer; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_heif_premultiplied_reference(
    bytes: *const u8,
    length: usize,
    offset_out: *mut usize,
    width_out: *mut usize,
    err_out: *mut *mut c_char,
) -> bool {
    if bytes.is_null() || offset_out.is_null() || width_out.is_null() {
        unsafe {
            set_err(
                err_out,
                "heif premultiplied reference needs bytes and both outputs",
            )
        };
        return false;
    }
    let file = unsafe { std::slice::from_raw_parts(bytes, length) };
    unsafe {
        fallible_op(
            err_out,
            "heif premultiplied reference",
            false,
            || premultiplied_reference(file),
            |found| {
                let (offset, width) = found.unwrap_or((0, 0));
                *offset_out = offset;
                *width_out = width;
                true
            },
        )
    }
}
