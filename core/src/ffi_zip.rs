//! The zipped Action bundle's exports (`zip_store`): pack a folder into one archive, and unpack
//! an archive into a folder. Paths in, `bool` out, the reason through `err_out`; every body runs
//! under `fallible_op`.

use std::ffi::c_char;
use std::path::Path;

use crate::ffi_util::{fallible_op, read_cstr};
use crate::zip_store;

/// Writes the regular files under `dir` into a new ZIP at `zip_path`, each named
/// `<dir's name>/<relative path>`, deterministically (sorted, fixed timestamps, deflate level 6),
/// replacing `zip_path` only with a complete archive. Refuses a symbolic link or special file and
/// a folder past the reader's caps. false with `err_out` on failure.
///
/// # Safety
/// `dir` and `zip_path` are NULL or NUL-terminated strings; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_zip_pack_dir(
    dir: *const c_char,
    zip_path: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "packing a zip archive",
            false,
            || {
                let dir = read_cstr(dir, "dir")?;
                let zip_path = read_cstr(zip_path, "zip_path")?;
                zip_store::pack_dir(Path::new(&dir), Path::new(&zip_path))
            },
            |()| true,
        )
    }
}

/// Extracts the ZIP at `zip_path` into the existing folder `dest_dir`, refusing (zip_store's
/// module doc) any entry that could land outside it, zip64, encryption, symbolic links, a method
/// other than stored or deflate, a CRC or size mismatch and an archive past its caps. On failure
/// `dest_dir` may hold entries written before it; the caller owns and removes it. false with
/// `err_out` on failure.
///
/// # Safety
/// `zip_path` and `dest_dir` are NULL or NUL-terminated strings; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_zip_unpack(
    zip_path: *const c_char,
    dest_dir: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "unpacking a zip archive",
            false,
            || {
                let zip_path = read_cstr(zip_path, "zip_path")?;
                let dest_dir = read_cstr(dest_dir, "dest_dir")?;
                zip_store::unpack(Path::new(&zip_path), Path::new(&dest_dir))
            },
            |()| true,
        )
    }
}
