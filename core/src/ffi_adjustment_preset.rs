//! C FFI for Photoshop adjustment preset files: one export,
//! [`dr_adjustment_preset_file_decode`], over `adjustment_preset_file`. The
//! host reads the file (it owns the open panel and the path); the core owns
//! every format rule and every refusal sentence.

use std::ffi::{c_char, CString};
use std::ptr;

use serde_json::json;

use crate::adjustment_preset_file::{self, MAX_FILE_BYTES};
use crate::ffi_util::{fallible_op, read_cstr};

/// Decodes the `len` bytes at `bytes` as a Photoshop adjustment preset file
/// of `extension` (no dot, any case: acv, alv, ahu, eap, blw or cha) and returns
/// `{"op":…,"params":{…}}` — the op and the params an adjustment layer of
/// that op stores — as a heap JSON string freed with `dr_string_free`.
/// NULL `bytes` with `len` 0 is an empty file (refused by its format's own
/// truncation sentence). NULL with a message through `err_out` on a NULL
/// `extension`, a NULL `bytes` with a non-zero `len`, `len` over 65536
/// (checked before the bytes are read), an unsupported extension, or any
/// file the format cannot carry; never panics.
///
/// # Safety
/// `bytes` must be NULL or readable for `len` bytes; `extension` must be NULL
/// or a valid NUL-terminated C string; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_adjustment_preset_file_decode(
    bytes: *const u8,
    len: usize,
    extension: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        let extension = unsafe { read_cstr(extension, "extension") }?;
        if len > MAX_FILE_BYTES {
            return Err(adjustment_preset_file::too_large(len));
        }
        let file: &[u8] = if bytes.is_null() {
            if len != 0 {
                return Err("bytes is NULL".to_string());
            }
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes, len) }
        };
        let (op, params) = adjustment_preset_file::decode(&extension, file)?;
        Ok(json!({"op": op, "params": params}).to_string())
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while decoding a Photoshop preset file",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}
