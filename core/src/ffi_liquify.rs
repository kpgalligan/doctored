//! C FFI for the Liquify workspace's live field: the five
//! `dr_liquify_field_*` exports over `liquify::LiquifyField`. The committed
//! edit does not come through here; it is `dr_image_filter_op` op `liquify`
//! at 8 bits and the selected edit at 16, which replay the same strokes.
//!
//! Thin shims over the shared `ffi_util` helpers: a field is an immutable
//! handle, `apply` returns a new one, and nothing unwinds across the boundary.
use std::ffi::c_char;

use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, handle_get};
use crate::job_control::JobContext;
use crate::liquify::{parse_strokes, LiquifyField, MAX_REQUEST_BYTES};
use crate::DrImage;

/// The identity field of a `width` × `height` plane. NULL with a message
/// through `err_out` when the plane is empty or past the pixel limit.
///
/// # Safety
/// `err_out` must be NULL or point to writable storage. Free the message with
/// dr_string_free and the field with dr_liquify_field_free.
#[no_mangle]
pub unsafe extern "C" fn dr_liquify_field_create(
    width: u32,
    height: u32,
    err_out: *mut *mut c_char,
) -> *mut LiquifyField {
    unsafe {
        fallible_op(
            err_out,
            "creating a liquify field",
            std::ptr::null_mut(),
            || LiquifyField::new((width, height)),
            boxed,
        )
    }
}

/// Pure: a new field with the strokes of `strokes_json` (the kernel JSON
/// `{"strokes":[…]}` of `dr_image_filter_op` op `liquify`, at most 8 MiB, in
/// plane pixels) applied after `field`'s own. `field` is left as it was,
/// refusal included. NULL `field` returns NULL with no error. Bad JSON, an
/// invalid stroke, or a stroke that takes the field's summed work past the
/// 1-billion limit returns NULL with a message through `err_out`.
///
/// # Safety
/// `field` must be NULL or a live field; `strokes_json` must be NULL or a
/// valid NUL-terminated C string; `err_out` must be NULL or point to writable
/// storage. Free the message with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_liquify_field_apply(
    field: *const LiquifyField,
    strokes_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut LiquifyField {
    unsafe {
        fallible_op(
            err_out,
            "applying liquify strokes",
            std::ptr::null_mut(),
            || {
                let Some(field) = field.as_ref() else {
                    return Ok(None);
                };
                let text = borrow_cstr_bounded(strokes_json, "liquify strokes", MAX_REQUEST_BYTES)?;
                let params = match serde_json::from_str(text)
                    .map_err(|e| format!("invalid liquify strokes: {e}"))?
                {
                    serde_json::Value::Object(map) => map,
                    _ => return Err("liquify strokes must be a JSON object".into()),
                };
                let strokes = parse_strokes(&params)?;
                let mut next = field.clone();
                next.apply(&strokes, JobContext::NONE)?;
                Ok(Some(next))
            },
            |field| field.map_or(std::ptr::null_mut(), boxed),
        )
    }
}

/// `img`, a picture of the field's plane at any size, rendered through the
/// field. NULL `previous`: all of `img` (the rect is not read). Otherwise a
/// copy of `previous` (the same size as `img`) with the rect `x, y, width,
/// height` re-rendered from `img`; an empty rect, one not inside `img`, or a
/// `previous` of another size errors. NULL `field` or `img` returns NULL with
/// no error.
///
/// # Safety
/// `field` must be NULL or a live field; `img` and `previous` must be NULL or
/// valid pointers to live `DrImage`s; `err_out` must be NULL or point to
/// writable storage. Free the message with dr_string_free.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_liquify_field_render(
    field: *const LiquifyField,
    img: *const DrImage,
    previous: *const DrImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a liquify field",
            std::ptr::null_mut(),
            || {
                let (Some(field), Some(image)) = (field.as_ref(), img.as_ref()) else {
                    return Ok(None);
                };
                let previous = previous.as_ref().map(|p| &p.pixels);
                field
                    .render_rgba8(&image.pixels, previous, (x, y, width, height))
                    .map(Some)
            },
            |pixels| pixels.map_or(std::ptr::null_mut(), |pixels| boxed(DrImage { pixels })),
        )
    }
}

/// The field's freeze as coverage 0…255, sampled at the pixel centres of a
/// `width` × `height` picture of the plane, written row-major to `out`. False,
/// with `out` untouched, for a NULL `field` or `out`, an empty picture, or
/// `out_len` other than `width·height`.
///
/// # Safety
/// `field` must be NULL or a live field; `out` must be NULL or valid for
/// `out_len` bytes of writes.
#[no_mangle]
pub unsafe extern "C" fn dr_liquify_field_freeze(
    field: *const LiquifyField,
    width: u32,
    height: u32,
    out: *mut u8,
    out_len: usize,
) -> bool {
    if out.is_null() || u64::from(width) * u64::from(height) != out_len as u64 {
        return false;
    }
    unsafe {
        handle_get(field, false, |field| {
            // The length is the core's own: `freeze` answers width·height
            // samples, which the check above matched to the caller's buffer.
            let Ok(mask) = field.freeze((width, height)) else {
                return false;
            };
            std::ptr::copy_nonoverlapping(mask.as_ptr(), out, mask.len());
            true
        })
    }
}

/// Frees a field. NULL is ignored.
///
/// # Safety
/// `field` must be NULL or an unfreed pointer returned by
/// dr_liquify_field_create or dr_liquify_field_apply.
#[no_mangle]
pub unsafe extern "C" fn dr_liquify_field_free(field: *mut LiquifyField) {
    unsafe { free_handle(field) }
}
