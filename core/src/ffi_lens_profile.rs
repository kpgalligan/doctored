//! C FFI for Lensfun lens profiles (v1-lensfun) — the "Lens profiles"
//! section of `include/doctored_core.h`: the lens identity an EXIF packet
//! names (`tiff_view::lens_identity_from_exif`) and the bundled database's
//! match and resolved profile (`lens_db::match_request`). Neither takes a
//! document; the profile they answer is `lens_correction`'s `profile`
//! parameter.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::ffi_util::{borrow_cstr_bounded, fallible_op};

/// The longest match request read: an identity's strings and numbers are a
/// few hundred bytes, so 1 MiB bounds the parse with room to spare.
const REQUEST_JSON_LIMIT: usize = 1024 * 1024;

/// A heap JSON string for the host.
fn json_string(value: &serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| e.to_string())
}

/// The lens identity read from an EXIF packet (`DR_METADATA_EXIF`: a TIFF
/// stream with no `Exif\0\0` prefix), as JSON `{"make","model","lens_make",
/// "lens_model": string|null, "focal_length","f_number","subject_distance",
/// "focal_length_35mm","pixel_width","pixel_height": number|null}`. NULL for a
/// NULL pointer, a zero length or a packet past the 16 MiB blob cap, with no
/// error; an unreadable packet answers every field null. Free with
/// `dr_string_free`.
///
/// # Safety
/// `exif` must be NULL or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_lens_exif_identity(exif: *const u8, len: usize) -> *mut c_char {
    if exif.is_null() || len == 0 || len > MAX_DRDC_BLOB_LEN as usize {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "reading a lens identity",
            ptr::null_mut(),
            || {
                // The length is the caller's packet length, checked above.
                let blob = std::slice::from_raw_parts(exif, len);
                json_string(&crate::tiff_view::lens_identity_from_exif(blob).to_json())
            },
            CString::into_raw,
        )
    }
}

/// The bundled Lensfun database's answer to `request_json`
/// `{"identity":{…as dr_lens_exif_identity…}, "lens":"auto"|"<lens id>",
/// "all":bool, "dimensions":[w,h]}` (`lens_db::match_request`): the database
/// credit, the camera, the crop factor and its source, the candidate lenses,
/// with `all` every lens on the camera's mount family, the selected lens, its
/// resolved profile and the reason there is none. Free with `dr_string_free`.
/// NULL with `err_out` set for a NULL or malformed request, an unknown key or
/// an unknown lens id; no match is not an error.
///
/// # Safety
/// `request_json` must be NULL or a NUL-terminated string; `err_out` must be
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_lens_match(
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "matching a lens profile",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(request_json, "request_json", REQUEST_JSON_LIMIT)?;
                let request: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("invalid lens match request: {e}"))?;
                json_string(&crate::lens_db::match_request(&request)?)
            },
            CString::into_raw,
        )
    }
}
