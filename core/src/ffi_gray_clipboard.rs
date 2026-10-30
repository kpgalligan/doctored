//! The Grayscale clipboard's two exports (#1 phase 4): a Gray payload's
//! one-channel TIFF for the pasteboard, and a payload made from another
//! app's gray TIFF on the pasteboard (`gray_clipboard`).

use std::ffi::c_char;
use std::ptr;

use crate::cmyk_clipboard_codec::AnyClipboardPayload;
use crate::ffi_clipboard::DrClipboardPayload;
use crate::ffi_util::{boxed, fallible_op, read_cstr, required_handle, write_string};
use crate::gray_clipboard::GrayClipboardPayload;
use crate::gray_working::GrayWorkingSpace;

/// A Gray payload's one-channel TIFF (`GrayClipboardPayload::gray_tiff`:
/// gray, plus alpha only when some pixel is not opaque, 8 bits for a depth-8
/// payload else 16, the Gray ICC embedded), made on the first call and
/// borrowed until the handle is freed. NULL with length 0 and an error for a
/// NULL handle, a payload that is not Gray, or a refusal.
///
/// # Safety
/// `payload` NULL or a live handle; `length_out` and `err_out` NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_gray_tiff(
    payload: *const DrClipboardPayload,
    limit: u64,
    other_retained_bytes: u64,
    length_out: *mut usize,
    err_out: *mut *mut c_char,
) -> *const u8 {
    unsafe {
        if !length_out.is_null() {
            *length_out = 0;
        }
        fallible_op(
            err_out,
            "writing a Grayscale clipboard TIFF",
            ptr::null(),
            || {
                let handle = required_handle(payload)?;
                let AnyClipboardPayload::Gray(gray) = &handle.payload else {
                    return Err("the clipboard payload is not Grayscale".into());
                };
                if handle.gray_tiff.get().is_none() {
                    let bytes = gray.gray_tiff(limit, other_retained_bytes)?;
                    // A racing first call made the same bytes; keep either.
                    let _ = handle.gray_tiff.set(bytes);
                }
                let bytes = handle
                    .gray_tiff
                    .get()
                    .ok_or("the Grayscale clipboard TIFF is missing")?;
                Ok((bytes.as_ptr(), bytes.len()))
            },
            |(pointer, length)| {
                if !length_out.is_null() {
                    *length_out = length;
                }
                pointer
            },
        )
    }
}

/// A Gray payload from a gray TIFF on the pasteboard
/// (`GrayClipboardPayload::from_gray_tiff`): its gray (and alpha) as
/// R = G = B at its depth (8 or 16) at offset (0, 0). NULL with NO error
/// when the bytes are not a gray TIFF `gray_io::gray_source` answers for.
/// An embedded Gray ICC is kept, and an unusable one is an error, as a gray
/// open refuses it; an untagged TIFF is assigned `fallback_icc` (NULL with
/// length 0 for none; kept only when it is a usable Gray profile), else the
/// working Gray `working_gray` (its wire name), and
/// `*assigned_out` (may be NULL; free with `dr_string_free`) receives the
/// untagged sentence naming it; it is set to NULL otherwise. Free the
/// payload with `dr_clipboard_payload_free`.
///
/// # Safety
/// `bytes` NULL or readable for `length` bytes; `fallback_icc` NULL or
/// readable for `fallback_length` bytes; `working_gray` NULL or
/// NUL-terminated; `assigned_out` and `err_out` NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_clipboard_payload_from_gray_image(
    bytes: *const u8,
    length: usize,
    fallback_icc: *const u8,
    fallback_length: usize,
    working_gray: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    assigned_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        if !assigned_out.is_null() {
            *assigned_out = ptr::null_mut();
        }
        fallible_op(
            err_out,
            "reading a gray image from the clipboard",
            ptr::null_mut(),
            || {
                if bytes.is_null() {
                    return Err("clipboard image bytes are NULL".into());
                }
                let working = read_cstr(working_gray, "working_gray")?;
                let working = GrayWorkingSpace::from_name(&working)
                    .ok_or_else(|| format!("\"{working}\" is not a Gray working space"))?;
                let input = std::slice::from_raw_parts(bytes, length);
                let fallback = if fallback_icc.is_null() {
                    None
                } else {
                    Some(std::slice::from_raw_parts(fallback_icc, fallback_length))
                };
                GrayClipboardPayload::from_gray_tiff(
                    input,
                    fallback,
                    working,
                    limit,
                    other_retained_bytes,
                )?
                .map(|(payload, assigned)| {
                    let handle = DrClipboardPayload::new(AnyClipboardPayload::Gray(payload))?;
                    Ok((handle, assigned))
                })
                .transpose()
            },
            |made| match made {
                Some((handle, assigned)) => {
                    if let Some(sentence) = assigned {
                        write_string(assigned_out, &sentence);
                    }
                    boxed(handle)
                }
                None => ptr::null_mut(),
            },
        )
    }
}
