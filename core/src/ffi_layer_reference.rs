//! C FFI for a byte layer's reference point: `dr_doc_layer_reference_x`
//! / `_y`, the `fxrp` a linked pattern fill layer's origin follows
//! (`DrDocument::layer_reference`). A typed document reports it in its
//! inspection row's `"reference"` (`precision_inspect`); and
//! `dr_turn_pattern`, a pattern fill layer's turned tile after a canvas turn
//! or flip (`doc_reference::turn_pattern`).

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_reference::turn_pattern;
use crate::ffi_util::{borrow_cstr_bounded, doc_get, fallible_op};

/// The layer's reference point x (`fxrp`, canvas px); 0 on NULL doc or
/// out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_reference_x(doc: *const DrDocument, idx: usize) -> i32 {
    unsafe { doc_get(doc, 0, |d| Some(d.layer_reference(idx)?.0)) }
}

/// The layer's reference point y (`fxrp`, canvas px); 0 on NULL doc or
/// out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_reference_y(doc: *const DrDocument, idx: usize) -> i32 {
    unsafe { doc_get(doc, 0, |d| Some(d.layer_reference(idx)?.1)) }
}

/// A pattern fill layer's tile after a canvas turn or flip
/// (`doc_reference::turn_pattern`): `pattern` is the fill's `pattern` object
/// as UTF-8 JSON (at most `artwork_document::MAX_LARGE_DESCRIPTION` bytes),
/// and (a, b, c, d) the turn's linear part, (x, y) -> (a x + c y, b x + d y).
/// Returns the turned pattern object as heap JSON, freed with
/// `dr_string_free`; NULL with no error when the part is not exactly a
/// quarter turn or a flip (the tile is kept); NULL with `err_out` set for
/// bad input.
///
/// # Safety
/// `pattern` must be NULL or a valid NUL-terminated C string; `err_out` must
/// be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_turn_pattern(
    pattern: *const c_char,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "dr_turn_pattern",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(
                    pattern,
                    "pattern",
                    crate::artwork_document::MAX_LARGE_DESCRIPTION,
                )?;
                let value: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| format!("invalid pattern: {e}"))?;
                turn_pattern(&value, [a, b, c, d])
            },
            |turned| {
                turned
                    .and_then(|value| CString::new(value.to_string()).ok())
                    .map_or(ptr::null_mut(), CString::into_raw)
            },
        )
    }
}
