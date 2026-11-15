//! The tile pool's exports (13C-1, `doc_tile_pool`): open and free the
//! handle, write a byte or typed document into it, rebuild a skeleton,
//! collect, and report. The handle owns files on disk; a write changes the
//! pool's files, never the document. Every body runs under `fallible_op`.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_tile_pool::{DrTilePool, TILE_POOL_WINDOW_BYTES};
use crate::ffi_util::{boxed, fallible_op, free_handle, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;

/// The most skeletons one collection may name.
const MAX_SKELETONS: usize = 4096;

/// Opens the pool directory, making it (one level, never its parents) when
/// absent. NULL with `err_out` on failure.
///
/// # Safety
/// `directory` is NULL or a NUL-terminated string; `err_out` is NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_tile_pool_open(
    directory: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrTilePool {
    unsafe {
        fallible_op(
            err_out,
            "opening the tile pool",
            ptr::null_mut(),
            || DrTilePool::open(&read_cstr(directory, "directory")?),
            boxed,
        )
    }
}

/// Frees the handle; its files stay on disk. NULL is a no-op.
///
/// # Safety
/// `pool` is NULL or an unfreed handle from `dr_tile_pool_open`.
#[no_mangle]
pub unsafe extern "C" fn dr_tile_pool_free(pool: *mut DrTilePool) {
    unsafe { free_handle(pool) }
}

/// Writes a byte document into the pool and its skeleton to
/// `skeleton_path`, atomically, after the pool's payloads are durable.
///
/// # Safety
/// `doc` and `pool` are NULL or live handles; `skeleton_path` is NULL or a
/// NUL-terminated string; `control` is NULL or a live single-use token;
/// `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_tile_pool_write(
    doc: *const DrDocument,
    pool: *mut DrTilePool,
    skeleton_path: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "writing a document into the tile pool",
            false,
            || {
                required_handle(doc)?.tile_pool_write(
                    required_handle(pool)?,
                    &read_cstr(skeleton_path, "skeleton path")?,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// The typed twin of `dr_doc_tile_pool_write`, admitted like the typed
/// native save plus the pool's window against `limit` beside `other`.
///
/// # Safety
/// As `dr_doc_tile_pool_write`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_tile_pool_write(
    doc: *const PrecisionDocument,
    pool: *mut DrTilePool,
    skeleton_path: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "writing a high-depth document into the tile pool",
            false,
            || {
                required_handle(doc)?.tile_pool_write(
                    required_handle(pool)?,
                    &read_cstr(skeleton_path, "skeleton path")?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// Rebuilds the native file a skeleton names into `native_path`,
/// atomically. A damaged skeleton or payload refuses by name.
///
/// # Safety
/// `pool` is NULL or a live handle; both paths are NULL or NUL-terminated
/// strings; `control` is NULL or a live single-use token; `err_out` is NULL
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_tile_pool_rebuild(
    pool: *mut DrTilePool,
    skeleton_path: *const c_char,
    native_path: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rebuilding from the tile pool",
            false,
            || {
                required_handle(pool)?.rebuild(
                    &read_cstr(skeleton_path, "skeleton path")?,
                    &read_cstr(native_path, "native path")?,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// Keeps only the payloads the `count` skeletons name; refuses, removing
/// nothing, when any of them cannot be read. `count` is at most 4096.
///
/// # Safety
/// `pool` is NULL or a live handle; `skeleton_paths` is NULL or `count`
/// readable pointers, each NULL or a NUL-terminated string; `err_out` is
/// NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_tile_pool_collect(
    pool: *mut DrTilePool,
    skeleton_paths: *const *const c_char,
    count: usize,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "collecting the tile pool",
            false,
            || {
                let pool = required_handle(pool)?;
                if count > MAX_SKELETONS {
                    return Err(format!(
                        "a collection names at most {MAX_SKELETONS} skeletons"
                    ));
                }
                if skeleton_paths.is_null() && count > 0 {
                    return Err("skeleton paths is NULL".into());
                }
                let mut paths = Vec::with_capacity(count);
                for index in 0..count {
                    paths.push(read_cstr(*skeleton_paths.add(index), "skeleton path")?);
                }
                pool.collect(&paths)
            },
            |()| true,
        )
    }
}

/// The pool's counts and the last write's as JSON (`tiles`, `packs`,
/// `stored_bytes`, `live_bytes`, `appended_tiles`, `appended_chunks`,
/// `appended_bytes`, `hashed_tiles`, `hashed_chunks`, `raw_payloads`,
/// `deflate_payloads`, `shuffle_payloads`), freed with `dr_string_free`.
///
/// # Safety
/// `pool` is NULL or a live handle; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_tile_pool_info(
    pool: *const DrTilePool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading the tile pool",
            ptr::null_mut(),
            || required_handle(pool)?.info_json(),
            |text| CString::new(text).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// The fixed transient a pool write or rebuild holds beside the document's
/// own plan (`TILE_POOL_WINDOW_BYTES`).
#[no_mangle]
pub extern "C" fn dr_tile_pool_window_bytes() -> u64 {
    TILE_POOL_WINDOW_BYTES
}
