//! The CMYK clipboard's two exports (#1 phase 4c): a payload's profile
//! bytes, which the host compares to decide whether a paste converts, and a
//! payload made from a CMYK TIFF or JPEG on the system pasteboard
//! (`CmykClipboardPayload::from_cmyk_image`).

use std::ffi::c_char;
use std::ptr;

use crate::cmyk_clipboard_codec::{AnyClipboardPayload, CmykClipboardPayload};
use crate::ffi_clipboard::DrClipboardPayload;
use crate::ffi_util::{boxed, fallible_op, write_string};

/// The payload's profile bytes — the RGB ICC profile, or the CMYK policy's
/// verbatim bytes — borrowed until the handle is freed; NULL with length 0
/// for a NULL handle.
///
/// # Safety
/// `payload` must be NULL or a live handle; `length_out` must be NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_icc(
    payload: *const DrClipboardPayload,
    length_out: *mut usize,
) -> *const u8 {
    unsafe {
        let (pointer, length) = match payload.as_ref() {
            Some(handle) => {
                let bytes = handle.payload.profile_bytes();
                (bytes.as_ptr(), bytes.len())
            }
            None => (ptr::null(), 0),
        };
        if !length_out.is_null() {
            *length_out = length;
        }
        pointer
    }
}

/// A CMYK payload from a CMYK TIFF or JPEG on the pasteboard
/// (`CmykClipboardPayload::from_cmyk_image`): its ink, declared depth and
/// resolution at offset (0, 0), its own profile when it embeds a usable one.
/// NULL with NO error when the bytes are not a CMYK TIFF or JPEG
/// `cmyk_source` answers for. When the image is untagged, `fallback_icc`
/// (NULL with length 0 for none) — else the working CMYK profile — is
/// assigned and `*assigned_out` (may be NULL; free with `dr_string_free`)
/// receives the untagged sentence naming it; it is set to NULL otherwise.
/// Free the payload with `dr_clipboard_payload_free`.
///
/// # Safety
/// `bytes` NULL or readable for `length` bytes; `fallback_icc` NULL or
/// readable for `fallback_length` bytes; `assigned_out` and `err_out` NULL
/// or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_clipboard_payload_from_cmyk_image(
    bytes: *const u8,
    length: usize,
    fallback_icc: *const u8,
    fallback_length: usize,
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
            "reading a CMYK image from the clipboard",
            ptr::null_mut(),
            || {
                if bytes.is_null() {
                    return Err("clipboard image bytes are NULL".into());
                }
                let input = std::slice::from_raw_parts(bytes, length);
                let fallback = if fallback_icc.is_null() {
                    None
                } else {
                    Some(std::slice::from_raw_parts(fallback_icc, fallback_length))
                };
                CmykClipboardPayload::from_cmyk_image(input, fallback, limit, other_retained_bytes)?
                    .map(|(payload, assigned)| {
                        let handle = DrClipboardPayload::new(AnyClipboardPayload::Cmyk(payload))?;
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
