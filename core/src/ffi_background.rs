//! C FFI for the Background mark (`doc_background`): `dr_doc_background_role`
//! and `dr_precision_doc_background_role`, the role the document's mark gives
//! its bottom entry. Nothing sets a mark through the FFI: only PSD import
//! does (`psd_background`).

use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, handle_get};
use crate::precision_document::PrecisionDocument;

/// The role of entry 0: 0 when no mark applies to it (D5 is the structural
/// rule), 1 Background, 2 ordinary layer. 0 on NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_background_role(doc: *const DrDocument) -> i32 {
    unsafe {
        doc_get(doc, 0, |d| {
            Some(d.background_role().map_or(0, |r| r.code() as i32))
        })
    }
}

/// The typed twin of [`dr_doc_background_role`], for root entry 0. 0 on NULL
/// doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `PrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_background_role(doc: *const PrecisionDocument) -> i32 {
    unsafe {
        handle_get(doc, 0, |d| {
            d.background_role().map_or(0, |r| r.code() as i32)
        })
    }
}
