//! Shared live-handle resource inventory and synchronized cache eviction:
//! `dr_mixed_resources_report_with_raw_masks` and
//! `dr_mixed_resources_evict_caches` (beside `dr_set_process_working_limit`
//! and `dr_process_cache_trim_limit`), both reading their handle lists
//! through `handles`. Handles are borrowed; neither export creates an
//! editable document or undo.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::fallible_op;
use crate::DrImage;

// Host registries are bounded defensively before pointer-array reads. This is
// not a document/layer capacity limit: it only caps one diagnostics request.
const MAX_HANDLES: usize = 65_536;

/// Reads a counted handle list. NULL entries are ignored. Array length cannot
/// be derived from dimensions, so this explicitly counted nonpixel API checks
/// a small fixed cap first; the host guarantees the array's actual extent.
pub(crate) unsafe fn handles<'a, T>(
    values: *const *const T,
    count: usize,
    kind: &str,
) -> Result<Vec<&'a T>, String> {
    if count > MAX_HANDLES {
        return Err(format!(
            "resource {kind} handle count exceeds {MAX_HANDLES}"
        ));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        return Err(format!("resource {kind} handle array is NULL"));
    }
    Ok(unsafe { std::slice::from_raw_parts(values, count) }
        .iter()
        .filter_map(|value| unsafe { value.as_ref() })
        .collect())
}

/// Counts unique retained heap payloads across byte/typed documents and
/// images, native local-mask handles and RBIN aliases — one shared allocation
/// union, so cross-depth masters count once — plus the supplied scratch
/// estimate. NULL entries are ignored; NULL arrays require count zero. A zero
/// working limit disables its comparison. Returns owned JSON, released with
/// dr_string_free, or NULL with err_out on invalid input/internal error.
///
/// # Safety
/// Each non-NULL array contains count readable pointers (at most 65536 each),
/// each non-NULL handle remains live through this call, and err_out is
/// NULL/writable. Concurrent immutable reads and synchronized cache access are
/// permitted; freeing any supplied handle concurrently is not.
#[no_mangle]
pub unsafe extern "C" fn dr_mixed_resources_report_with_raw_masks(
    documents: *const *const DrDocument,
    document_count: usize,
    images: *const *const DrImage,
    image_count: usize,
    typed_documents: *const *const crate::precision_document::PrecisionDocument,
    typed_document_count: usize,
    typed_images: *const *const crate::precision_image::PrecisionImage,
    typed_image_count: usize,
    masks: *const *const crate::raw_local_mask::RawLocalMask,
    mask_count: usize,
    transient: u64,
    limit: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reporting local-mask resource inventory",
            ptr::null_mut(),
            || {
                let docs = handles(documents, document_count, "document")?;
                let images = handles(images, image_count, "image")?;
                let typed = handles(typed_documents, typed_document_count, "precision document")?;
                let typed_images = handles(typed_images, typed_image_count, "precision image")?;
                let masks = handles(masks, mask_count, "local mask")?;
                CString::new(
                    crate::resources::mixed_report_with_masks(
                        &docs,
                        &images,
                        &typed,
                        &typed_images,
                        &masks,
                        transient,
                        limit,
                    )
                    .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Evicts the disposable caches reachable from the supplied documents (style
/// and mask planes, process-wide LUT and content-box memos, contiguous layer
/// views), never pixels, document state or history, and reports
/// `{"cache_bytes_before","cache_bytes_after","removed_cache_bytes"}` over
/// both owner families as owned JSON (free with `dr_string_free`); NULL with
/// err_out on invalid input or an internal error. Removed planes stay alive
/// until concurrent readers finish.
///
/// # Safety
/// Each non-NULL array contains count readable pointers (at most 65536 each),
/// NULL entries are ignored, each supplied handle remains live through this
/// call, and err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_mixed_resources_evict_caches(
    documents: *const *const DrDocument,
    count: usize,
    typed: *const *const crate::precision_document::PrecisionDocument,
    typed_count: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "evicting resource caches",
            ptr::null_mut(),
            || {
                let documents = handles(documents, count, "document")?;
                let typed = handles(typed, typed_count, "precision document")?;
                let inventory = |documents: &[&DrDocument]| {
                    let mut inventory = crate::resources::Inventory::default();
                    for d in documents {
                        inventory.document(d);
                    }
                    for d in &typed {
                        d.record_resources(&mut inventory);
                    }
                    crate::resources::cache_inventory(documents, &mut inventory);
                    inventory.cache_bytes()
                };
                let before = inventory(&documents);
                crate::resources::evict_caches(&documents);
                let after = inventory(&documents);
                let value = serde_json::json!({
                    "cache_bytes_before": before,
                    "cache_bytes_after": after,
                    "removed_cache_bytes": before.saturating_sub(after),
                });
                CString::new(value.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Sets the process-wide working limit, used wherever no caller limit is
/// passed (internal constructors, `*_plan()` defaults, the native reopen
/// check, the PSD layered-import envelope). The default is 2 GiB. Returns
/// false for 0 and leaves the value unchanged. Call once at launch;
/// thread-safe. An `admit` with an explicit caller limit never consults it.
#[no_mangle]
pub extern "C" fn dr_set_process_working_limit(bytes: u64) -> bool {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "setting process working limit",
            false,
            || Ok(crate::precision_image::set_process_working_limit(bytes)),
            |value| value,
        )
    }
}

/// The host's post-projection trim threshold for non-view cache bytes (the
/// disposable caches `dr_mixed_resources_evict_caches` drops): the process
/// working limit / 8 (`cache_limit.rs`), so it follows
/// `dr_set_process_working_limit`. 0 on an
/// internal error. It takes no pointer, so the null-safety sweep has nothing
/// to add for it.
#[no_mangle]
pub extern "C" fn dr_process_cache_trim_limit() -> u64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "reading process cache trim limit",
            0,
            || Ok(crate::precision_image::process_cache_trim_limit()),
            |value| value,
        )
    }
}
