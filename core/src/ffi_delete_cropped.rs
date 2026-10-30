//! FFI shim for Delete Cropped Pixels (`doc_delete_cropped`).

use crate::doc::DrDocument;
use crate::ffi_util::doc_op;

/// A new document with every eligible raster layer (raster, no meta, no
/// retained source) trimmed to the canvas; a layer wholly outside it becomes
/// a 1×1 transparent pixel at (0, 0). Masks, groups, described layers,
/// channels, guides and samplers are untouched; layer comps bound to a
/// trimmed layer shift with its origin. NULL on NULL `doc`, and when no
/// eligible layer reaches outside the canvas.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_delete_cropped_pixels(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::delete_cropped_pixels) }
}
