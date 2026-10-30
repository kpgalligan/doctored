//! Typed metadata reads. Borrowed byte slices remain owned by the immutable
//! document; no preview, working-space adoption or pixel conversion is involved.
//!
//! The public Swift facade uses one typed owner and never routes a typed pointer to the legacy ABI.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc_color::MetadataKind;
use crate::doc_lock::EditKind;
use crate::ffi_util::{fallible_op, handle_get, required_handle};
use crate::precision_document::{Content, PrecisionDocument};

/// Borrow the document ICC bytes; NULL document/length returns NULL. Length
/// becomes zero on missing input. The document must outlive the returned slice.
/// # Safety
/// document is NULL/live; length is NULL/writable; returned bytes are read-only.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_profile(
    document: *const PrecisionDocument,
    length: *mut usize,
) -> *const u8 {
    if length.is_null() {
        return ptr::null();
    }
    unsafe {
        *length = 0;
        handle_get(document, ptr::null(), |d| {
            let bytes = d.profile().bytes();
            *length = bytes.len();
            bytes.as_ptr()
        })
    }
}

/// Borrow EXIF/XMP/IPTC (kind0/1/2); missing/invalid input returns NULL and
/// length0. The immutable document owns the returned bytes.
/// # Safety
/// document is NULL/live; length is NULL/writable; returned bytes are read-only.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_metadata(
    document: *const PrecisionDocument,
    kind: u32,
    length: *mut usize,
) -> *const u8 {
    if length.is_null() {
        return ptr::null();
    }
    unsafe {
        *length = 0;
        handle_get(document, ptr::null(), |d| {
            let bytes = match MetadataKind::from_c(kind as i32) {
                Some(MetadataKind::Exif) => d.metadata().exif.as_deref(),
                Some(MetadataKind::Xmp) => d.metadata().xmp.as_deref(),
                Some(MetadataKind::Iptc) => d.metadata().iptc.as_deref(),
                None => None,
            };
            bytes.map_or(ptr::null(), |bytes| {
                *length = bytes.len();
                bytes.as_ptr()
            })
        })
    }
}

/// Copy one canonical typed adjustment description; non-adjustment/invalid
/// index returns NULL without error. Free successful output with dr_string_free.
/// # Safety
/// document is NULL/live; err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_adjustment_description(
    document: *const PrecisionDocument,
    index: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading high-depth adjustment",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                match document.layer(index).map(|l| &l.content) {
                    Some(Content::Adjustment(state)) => CString::new(state.description())
                        .map(Some)
                        .map_err(|e| e.to_string()),
                    Some(Content::CmykAdjustment(state)) => CString::new(state.description())
                        .map(Some)
                        .map_err(|e| e.to_string()),
                    _ => Ok(None),
                }
            },
            |value| value.map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// Which of typed record `idx`'s effective lock bits — its own and every
/// enclosing group's — would block an edit of `kind` (0 = allowed) —
/// `dr_doc_lock_block`'s twin at high depth. The host asks
/// this to phrase a refusal that names the lock; the ops themselves refuse
/// regardless. 0 on a NULL document, an out-of-range index and an
/// unrecognized kind.
///
/// # Safety
/// document is NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_lock_block(
    document: *const PrecisionDocument,
    idx: usize,
    kind: i32,
) -> u32 {
    unsafe {
        handle_get(document, 0, |d| {
            EditKind::from_c(kind).map_or(0, |kind| d.lock_block(idx, kind))
        })
    }
}

/// The typed record whose OWN lock bits hold every bit of `locks` — `idx`
/// itself, else its nearest enclosing group that does — `dr_doc_lock_owner`'s
/// twin at high depth. False, with nothing written, on a NULL document,
/// an empty `locks`, an out-of-range index, or when no record in the chain
/// holds them.
///
/// # Safety
/// document is NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_idx` is NULL or a valid pointer to a writable `size_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_lock_owner(
    document: *const PrecisionDocument,
    idx: usize,
    locks: u32,
    out_idx: *mut usize,
) -> bool {
    let owner = unsafe { handle_get(document, None, |d| d.lock_owner(idx, locks)) };
    let Some(owner) = owner else {
        return false;
    };
    if !out_idx.is_null() {
        unsafe { *out_idx = owner };
    }
    true
}

/// The typed record's CONTENT bounds — `dr_doc_layer_bounds`' twin at high
/// depth, 16-bit and 32-bit float: the canvas box of its opaque
/// pixels with its enabled mask applied, a group's union over its raster
/// descendants, written to `out_xywh` as x, y, width, height. `idx` is the
/// postorder record index. False (nothing written) on a NULL document, an
/// out-of-range index, an adjustment record or nothing opaque; a NULL
/// `out_xywh` still answers.
///
/// # Safety
/// document is NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_xywh` is NULL or a valid pointer to four writable `int32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_bounds(
    document: *const PrecisionDocument,
    idx: usize,
    out_xywh: *mut i32,
) -> bool {
    let bounds = unsafe { handle_get(document, None, |d| d.layer_bounds(idx)) };
    let Some((x, y, w, h)) = bounds else {
        return false;
    };
    if !out_xywh.is_null() {
        let values = [
            x,
            y,
            i32::try_from(w).unwrap_or(i32::MAX),
            i32::try_from(h).unwrap_or(i32::MAX),
        ];
        for (i, &value) in values.iter().enumerate() {
            unsafe { *out_xywh.add(i) = value };
        }
    }
    true
}

/// `dr_doc_layer_at`'s twin at high depth, 16-bit and 32-bit float:
/// the postorder index of the record a click at canvas `(x, y)` activates —
/// the topmost visible raster record whose clamped alpha times its enabled
/// mask's coverage there is at least one half (an unpreparable feather
/// counts as no mask; exact byte parity for a 16-bit promotion at density 1
/// without feather) — written to `out_idx`. With `top_level` the answer is
/// that record's top-level ancestor. False (nothing written) on a NULL
/// document or when nothing is hit; a NULL `out_idx` still answers.
///
/// # Safety
/// document is NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_idx` is NULL or a valid pointer to a writable `size_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_at(
    document: *const PrecisionDocument,
    x: i32,
    y: i32,
    top_level: bool,
    out_idx: *mut usize,
) -> bool {
    let hit = unsafe { handle_get(document, None, |d| d.layer_at(x, y, top_level)) };
    let Some(hit) = hit else {
        return false;
    };
    if !out_idx.is_null() {
        unsafe { *out_idx = hit };
    }
    true
}
