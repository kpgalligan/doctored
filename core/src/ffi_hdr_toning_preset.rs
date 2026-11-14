//! C FFI for HDR Toning preset files (`.hdt`): one export,
//! [`dr_hdr_toning_preset_file_decode`], over `hdr_toning_preset_file`. The
//! host reads the file and names the preset (it owns the open panel, the path
//! and the library); the core owns every format rule and every refusal
//! sentence.

use std::ffi::{c_char, CString};
use std::ptr;

use serde_json::json;

use crate::ffi_util::fallible_op;
use crate::hdr_toning_preset_file::{self, MAX_FILE_BYTES};

/// Decodes the `len` bytes at `bytes` as an HDR Toning preset file (`.hdt`)
/// and returns `{"hdr_toning":{…},"report":[…]}` as a heap JSON string freed
/// with `dr_string_free`: `hdr_toning` is the Local Adaptation arguments the
/// file holds (`method`, every value and `toning_curve` as `[in, out]`
/// pairs), and each `report` line names a value imported other than as
/// stored (a gamma above 2 imports as 2; corner points import as smooth
/// ones). NULL `bytes` with `len` 0 is an empty file (refused by the format's
/// truncation sentence). NULL with a message through `err_out` on a NULL
/// `bytes` with a non-zero `len`, `len` over 4096 (checked before the bytes
/// are read), a method other than Local Adaptation, or any file that does not
/// parse exactly or holds a value outside the request's ranges; never panics.
///
/// # Safety
/// `bytes` must be NULL or readable for `len` bytes; `err_out` must be NULL or
/// a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_hdr_toning_preset_file_decode(
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        if len > MAX_FILE_BYTES {
            return Err(hdr_toning_preset_file::too_large(len));
        }
        let file: &[u8] = if bytes.is_null() {
            if len != 0 {
                return Err("bytes is NULL".to_string());
            }
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes, len) }
        };
        let decoded = hdr_toning_preset_file::decode(file)?;
        Ok(json!({"hdr_toning": decoded.arguments, "report": decoded.report}).to_string())
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while decoding an HDR Toning preset file",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}
