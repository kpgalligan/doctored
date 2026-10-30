//! C FFI for the composite's grey in a Gray working space
//! (`dr_doc_composite_gray_plane`, `doc_gray_plane`) — beside
//! `dr_doc_composite_plane` in the "Channels" section of
//! `include/doctored_core.h`. NULL tolerance first, the query through
//! `ffi_util::doc_get`, the output length recomputed from the core's own
//! dimensions.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, read_cstr};
use crate::gray_working::GrayWorkingSpace;

/// The flattened composite's grey in a Gray working space (`"dot_gain_10"`
/// … `"dot_gain_30"`, `"gray_gamma_18"`, `"gray_gamma_22"`): `v = Y^e`, Y
/// the document profile's relative PCS Y of the encoded RGB (matrix/TRC
/// model, else LUT), e = ln 0.5 / ln(0.5 − d) or 1/gamma; `out` = round(255
/// v), `w * h` bytes, row 0 top. false on NULL, a dimension mismatch, an
/// unknown name, or a profile with neither a matrix/TRC model nor a LUT.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `gray_space`
/// NULL or a NUL-terminated string; `out` NULL or writable for canvas
/// width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_composite_gray_plane(
    doc: *const DrDocument,
    gray_space: *const c_char,
    out: *mut u8,
    w: u32,
    h: u32,
) -> bool {
    if out.is_null() || gray_space.is_null() {
        return false;
    }
    let Ok(name) = (unsafe { read_cstr(gray_space, "gray_space") }) else {
        return false;
    };
    let plane = unsafe {
        doc_get(doc, None, |d| {
            if w != d.width || h != d.height {
                return None;
            }
            let space = GrayWorkingSpace::from_name(&name)?;
            let plane = d.composite_gray_plane(space)?;
            // The core's own canvas size, never the caller's.
            let len = (d.width as usize).checked_mul(d.height as usize)?;
            (plane.len() == len).then_some(Some(plane))
        })
    };
    let Some(plane) = plane else {
        return false;
    };
    unsafe { ptr::copy_nonoverlapping(plane.as_ptr(), out, plane.len()) };
    true
}
