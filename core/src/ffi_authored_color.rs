//! C FFI for the authored-colour decision (`dr_authored_color`), so the
//! host's brush, fill, gradient and text ink on a LUT-tagged document land on
//! the numbers the core's styles and artwork draw (`authored_color`). Same
//! conventions as the sibling shims: NULL tolerance first, a caught panic is
//! a refusal, and the three-float arrays are fixed-length by contract.

use std::panic::catch_unwind;

use crate::authored_color::authored_color;
use crate::ffi_color::profile_arg;

/// Converts one AUTHORED straight RGB triple between sRGB and the RGB LUT
/// profile `profile` (`len` bytes) through the core's relative-colorimetric,
/// BPC-off pipeline, the one layer styles, artwork and high-depth sRGB ink
/// take on such a document. `to_document` true: sRGB → profile; false:
/// profile → sRGB. `rgb` and `out` are 3 floats in 0…1 (input clamped).
/// False, `out` untouched, on NULL, non-RGB bytes, a profile with a
/// matrix/TRC model (the host converts those itself) or tables that cannot
/// convert that way.
///
/// # Safety
/// `profile` must be NULL or valid for `len` bytes; `rgb` must be NULL or
/// valid for reading 3 floats; `out` must be NULL or valid for writing 3
/// floats.
#[no_mangle]
pub unsafe extern "C" fn dr_authored_color(
    profile: *const u8,
    len: usize,
    to_document: bool,
    rgb: *const f32,
    out: *mut f32,
) -> bool {
    if profile.is_null() || rgb.is_null() || out.is_null() {
        return false;
    }
    let input: [f32; 3] = {
        let slice = unsafe { std::slice::from_raw_parts(rgb, 3) };
        [slice[0], slice[1], slice[2]]
    };
    let converted = catch_unwind(|| {
        let profile = unsafe { profile_arg(profile, len) }?;
        authored_color(&profile, to_document, input)
    })
    .unwrap_or(None);
    let Some(value) = converted else {
        return false;
    };
    let target = unsafe { std::slice::from_raw_parts_mut(out, 3) };
    target.copy_from_slice(&value);
    true
}
