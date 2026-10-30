//! FFI for a Gradient Fill layer's Align with Layer box
//! (`doc_fill_align`, the rule in `fill_align_box`): one export per document
//! kind, shaped like `dr_doc_layer_bounds` / `dr_precision_doc_layer_bounds`.

use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, handle_get};
use crate::precision_document::PrecisionDocument;

/// Writes `xywh` to `out` when it is not NULL.
///
/// # Safety
/// `out` must be NULL or valid for four `int32_t` writes.
unsafe fn write_box(out: *mut i32, xywh: [i32; 4]) {
    if out.is_null() {
        return;
    }
    for (i, &value) in xywh.iter().enumerate() {
        unsafe { *out.add(i) = value };
    }
}

/// Entry `idx`'s Align with Layer box — the bounds of its enabled mask's
/// coverage with outside coverage 0, moved to the canvas and clipped to it,
/// else the canvas — written to `out_xywh` as x, y, width, height. False (and
/// nothing written) on a NULL doc or an out-of-range idx; a NULL `out_xywh`
/// still answers.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `out_xywh`
/// must be NULL or valid for four `int32_t` writes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_fill_align_box(
    doc: *const DrDocument,
    idx: usize,
    out_xywh: *mut i32,
) -> bool {
    let found = unsafe { doc_get(doc, None, |d| Some(d.fill_align_box(idx))) };
    let Some(xywh) = found else {
        return false;
    };
    unsafe { write_box(out_xywh, xywh) };
    true
}

/// `dr_doc_fill_align_box`'s typed twin (16-bit, 32-bit float and CMYK) over
/// postorder record `idx` and its u16 mask plane. False (nothing written) on
/// a NULL document or an out-of-range index; a NULL `out_xywh` still answers.
///
/// # Safety
/// `document` is NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_xywh` is NULL or a valid pointer to four writable `int32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_fill_align_box(
    document: *const PrecisionDocument,
    idx: usize,
    out_xywh: *mut i32,
) -> bool {
    let found = unsafe { handle_get(document, None, |d| d.fill_align_box(idx)) };
    let Some(xywh) = found else {
        return false;
    };
    unsafe { write_box(out_xywh, xywh) };
    true
}
