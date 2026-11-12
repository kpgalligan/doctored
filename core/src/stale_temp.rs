//! The core's temporary-file name and the sweep of the temporaries a dead
//! process left behind.
//!
//! `dr_image::save_atomically` writes every file at `<path>.dr-tmp-<pid>-<seq>`
//! ([`name`]) beside its destination and renames it into place, removing it on
//! any failure. A process that is killed or loses power mid-write cannot remove
//! it, so before each write [`sweep`] lists the destination's folder and removes
//! the temporaries whose process no longer exists. The sweep is folder-wide,
//! not keyed on the destination's name, because the app hands the core its own
//! staged path (`.doctored-save-<pid>-<UUID>`), so a killed save leaves
//! `<that>.dr-tmp-<pid>-<seq>`, a name no later save of the same document
//! repeats.
//!
//! A name is removed only when it ends `.dr-tmp-<pid>-<seq>` exactly (both
//! decimal digits), it is a regular file (never a directory or a symlink), its
//! pid is not this process's, and `kill(pid, 0)` answers `ESRCH`: no such
//! process. `EPERM` (another user's live process) and every other answer keep
//! it, as does a pid outside `1..=i32::MAX`, so `kill` is never handed 0 or a
//! negative number (a process group). Every error is ignored: the sweep never
//! fails a save.
//!
//! A reused pid keeps its stale file until that process exits, the safe
//! direction. Two limits are accepted: a user's own file that happens to end
//! `.dr-tmp-<n>-<n>` with a dead pid is removed (the suffix is specific enough),
//! and on a folder shared between machines another machine's pid reads as dead
//! here, so its in-flight temporary can be removed while it writes; that save
//! then fails at its rename with the previous file intact.

use std::path::Path;

/// The infix every core temporary carries between the destination's name and
/// `<pid>-<seq>`.
const INFIX: &str = ".dr-tmp-";

/// The temporary path `save_atomically` writes `path` at: this process's pid
/// and `seq`, a process-wide sequence number, keep concurrent saves distinct.
pub(crate) fn name(path: &str, seq: u64) -> String {
    format!("{path}{INFIX}{}-{seq}", std::process::id())
}

/// Removes, best effort, every regular file in `dir` named like a core
/// temporary whose process no longer exists (the module doc gives the rule).
pub(crate) fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let own = std::process::id();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(temporary_pid) else {
            continue;
        };
        if u32::try_from(pid) == Ok(own) {
            continue;
        }
        let path = entry.path();
        let regular = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file());
        if regular && dead(pid) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// The pid of a name ending `.dr-tmp-<pid>-<seq>`, when the pid is in
/// `1..=i32::MAX` and both fields are decimal digits only.
fn temporary_pid(file_name: &str) -> Option<i32> {
    let (_, tail) = file_name.rsplit_once(INFIX)?;
    let (pid, seq) = tail.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(pid) || !digits(seq) {
        return None;
    }
    pid.parse::<i32>().ok().filter(|&pid| pid > 0)
}

/// Whether no process has this pid: `kill(pid, 0)` failed with `ESRCH`.
#[cfg(unix)]
fn dead(pid: i32) -> bool {
    // SAFETY: signal 0 sends nothing; it only asks whether `pid` exists, and
    // `pid` is positive, so it names one process, never a group.
    let answer = unsafe { libc::kill(pid, 0) };
    answer == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// No liveness query off Unix, so nothing is ever swept there.
#[cfg(not(unix))]
fn dead(_: i32) -> bool {
    false
}
