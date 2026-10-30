//! The core's one flush policy for published files: a BARRIER
//! flush of the finished temporary before the publishing rename, so the
//! rename can never reach the disk ahead of the bytes it names, and a
//! best-effort FULL flush of the parent directory after it, so the rename
//! itself survives power loss. `dr_image::save_atomically` is the only
//! caller; every core file writer goes through it.
//!
//! On Apple platforms `fsync(2)` only hands the data to the drive, whose
//! cache may still reorder or lose it; `F_BARRIERFSYNC` orders it and
//! `F_FULLFSYNC` forces it to stable storage. Either `fcntl` can fail on a
//! file system that does not implement it (SMB, FUSE, exFAT). The fallback
//! is SQLite's: plain `fsync`, whose error is the one reported, so those
//! volumes keep saving with the durability they can offer.
//!
//! `order_file` barrier-flushes (`F_BARRIERFSYNC`) a finished temporary before its publishing
//! rename, and `persist_parent_dir` best-effort full-flushes (`F_FULLFSYNC`) the parent directory
//! after it; an unsupported `fcntl` falls back to `fsync` (SQLite's rule).

use std::fs::File;
use std::path::Path;

/// Barrier-flushes `file` before it is renamed into place.
#[cfg(target_vendor = "apple")]
pub(crate) fn order_file(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    flush(file.as_raw_fd(), libc::F_BARRIERFSYNC)
}

/// Barrier-flushes `file` before it is renamed into place.
#[cfg(not(target_vendor = "apple"))]
pub(crate) fn order_file(file: &File) -> std::io::Result<()> {
    file.sync_all()
}

/// Full-flushes the directory holding `path` after a rename into it. Best
/// effort: a directory this process cannot open (write-only permissions, for
/// one) or flush leaves the already-published file in place and the save
/// successful.
pub(crate) fn persist_parent_dir(path: &str) {
    let directory = match Path::new(path).parent() {
        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
        Some(parent) => parent,
        None => return,
    };
    let Ok(handle) = File::open(directory) else {
        return;
    };
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        let _ = flush(handle.as_raw_fd(), libc::F_FULLFSYNC);
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = handle.sync_all();
    }
}

/// `fcntl(fd, command)`, retried on `EINTR`; any other failure falls back to
/// `fsync(fd)` (also retried on `EINTR`), and only fsync's error propagates.
#[cfg(target_vendor = "apple")]
fn flush(fd: std::os::fd::RawFd, command: libc::c_int) -> std::io::Result<()> {
    loop {
        // SAFETY: `fd` is an open descriptor borrowed from a live `File`.
        if unsafe { libc::fcntl(fd, command) } != -1 {
            return Ok(());
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            break;
        }
    }
    loop {
        // SAFETY: as above.
        if unsafe { libc::fsync(fd) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
}
