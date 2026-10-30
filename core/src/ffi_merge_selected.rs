//! The byte Stamp Selected and Merge Group exports (#1 phase 4c), the
//! shims of `doc_merge_selected` (whose module doc is the rule). The typed
//! twins are the `stamp_selected` and `merge_group` structure edits of
//! `dr_precision_doc_edit`.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{doc_op, index_slice, read_cstr};

/// Stamp Selected: ONE group, or two or more entries sharing one parent,
/// composited over their union extent into ONE new raster entry (Normal,
/// opacity 1, unclipped) directly above the topmost member's subtree at its
/// depth, named `name`; every original is untouched. NULL on NULL doc or
/// name, an empty, out-of-range or repeating list, one entry that is not a
/// group, entries that do not share one parent, or a union with nothing
/// visible.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `idx` must be
/// NULL or valid for `len` `size_t` values; `name` must be NULL or a valid
/// NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_stamp_selected(
    doc: *const DrDocument,
    idx: *const usize,
    len: usize,
    name: *const c_char,
) -> *mut DrDocument {
    let Ok(name) = (unsafe { read_cstr(name, "name") }) else {
        return ptr::null_mut();
    };
    unsafe {
        doc_op(doc, |d| {
            d.stamp_selected(&index_slice(idx, len, d.layers.len())?, &name)
        })
    }
}

/// Merge Group: the visible group `idx` replaced by ONE raster entry holding
/// its raw projection over its union extent, keeping the group's slot, name,
/// id, label, opacity, blend mode (Pass Through becomes Normal), style, mask,
/// clipping, locks and link. NULL on NULL doc, an out-of-range index, an
/// entry that is not a visible group, locks that refuse a merge, or a group
/// with nothing visible.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merge_group(doc: *const DrDocument, idx: usize) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.merge_group(idx)) }
}
