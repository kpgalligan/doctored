//! C FFI for stable layer identity — `dr_doc_layer_id`,
//! `dr_doc_layer_storage_identity` and the two mask-storage identities
//! (`dr_doc_layer_mask_storage_identity` /
//! `dr_precision_doc_layer_mask_storage_identity`), the "Layers" section of
//! `include/doctored_core.h`.
//!
//! Same conventions as `ffi_guide` and `ffi_channel`, and the same deliberate
//! absence of an `err_out`: the one refusal is "no such entry", which answers
//! 0 — a value no live layer ever carries, because `doc_identity`'s counter
//! starts at 1.
//!
//! **Two exports, deliberately.** `dr_doc_layer_id` is the layer's stable
//! identity; `dr_doc_layer_storage_identity` is the second, content-level
//! identity `doc_identity`'s module doc describes — what a host keys a
//! per-layer render cache on. The reverse lookup (`layer_index_by_id`) is
//! not exported: a host that holds an id also holds the document and can walk
//! `dr_doc_layer_count` ids once per change, exactly as it already caches the
//! guide list, and adding another call would put the same search in two
//! places. The typed depth answers the layer id through
//! `dr_precision_doc_info`'s `id` row rather than through a shim of its
//! own, for the reason that whole inspection exists: one bounded metadata
//! read per document change; its storage identity is
//! `dr_precision_image_storage_identity` on `dr_precision_doc_layer_source`.
//!
//! The MASK identity has an export at each depth: a typed mask has no
//! handle of its own to ask, and reading one through the scalar-plane path
//! would copy the plane the identity exists to avoid copying.

use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, handle_get};
use crate::precision_document::PrecisionDocument;

/// Entry `idx`'s STABLE IDENTITY — unique among every layer this process has
/// minted, kept through a rename, a reorder, an opacity change, a geometry op
/// and undo/redo, and re-minted on load. 0 on a NULL doc or an out-of-range
/// index, which no live layer ever answers.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_id(doc: *const DrDocument, idx: usize) -> u64 {
    // The `catch_unwind` and the NULL tolerance are `doc_get`'s, the same
    // helper every sibling reader in `ffi_guide` / `ffi_channel` routes
    // through.
    unsafe { doc_get(doc, 0, |d| d.layer_id(idx)) }
}

/// Opaque identity of entry `idx`'s pixel STORAGE. Equal nonzero values hold
/// the same pixels (a clone, an undo handle, a name/opacity/visibility/offset
/// write); every pixel write answers a new value, even one that leaves every
/// byte equal. Compare only while both rasters are owned; never dereference.
/// 0 for a NULL doc, an out-of-range index or a GROUP. Reads no pixel,
/// allocates nothing.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_storage_identity(doc: *const DrDocument, idx: usize) -> u64 {
    unsafe { doc_get(doc, 0, |d| d.layer_storage_identity(idx)) }
}

/// Opaque identity of entry `idx`'s raw MASK storage. Equal nonzero values
/// hold the same mask plane (a clone, an undo handle, a rename, a mask
/// density/feather/link/enabled write); painting or replacing the mask
/// answers a new value. Compare only while both planes are owned; never
/// dereference. 0 for a NULL doc, an out-of-range index, an entry with no
/// mask or a GROUP. Reads no pixel, allocates nothing.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_storage_identity(
    doc: *const DrDocument,
    idx: usize,
) -> u64 {
    unsafe { doc_get(doc, 0, |d| d.layer_mask_storage_identity(idx)) }
}

/// The typed twin of [`dr_doc_layer_mask_storage_identity`] for record `idx`
/// (the postorder numbering every `dr_precision_doc_*` index uses): the same
/// contract over the u16 mask plane. 0 for a NULL document, an out-of-range
/// index, a record with no mask or a group.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_mask_storage_identity(
    document: *const PrecisionDocument,
    idx: usize,
) -> u64 {
    unsafe {
        handle_get(document, 0, |d| {
            d.layer_mask_storage_identity(idx).unwrap_or(0)
        })
    }
}
