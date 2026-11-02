//! Launching Doctored for a `tools/call` that found no socket (MC-4,
//! `designs/agent-socket.md`): its own bundle, in the background (`open -n -g`), at most one
//! instance between all the proxies that find no socket at once.
//!
//! **The lock.** `agent.launch.lock` beside the socket, taken with
//! `File::lock` (exclusive, blocking) before the re-check and **held until
//! the poll ends**: released before the poll, a second proxy's re-check
//! would find nothing answering yet and launch a second instance. A proxy
//! that waited on the lock re-checks and finds the first one's instance.
//!
//! **A scratch socket** (`DR_AGENT_SOCKET` set, MC-6) is passed to the
//! instance with `open --env`, together with every other `DR_*` variable and
//! `ANTHROPIC_API_KEY` this process has (`forwarded_environment`: the
//! isolation a test set for the helper reaches the app it launches), with
//! `-ApplePersistenceIgnoreState YES` and its stderr written to
//! `doctored-launch.log` beside the socket; the person's own socket gets
//! none of these, so their Doctored opens as theirs.
//!
//! **The launch marker.** Every launch, scratch or the person's, ends
//! `--args … -DRMCPHelperLaunch YES` (`HELPER_LAUNCH_ARGUMENT`): it tells the
//! app that no person opened it, so its updater (`AppUpdater`) waits for the
//! first activation instead of asking anything of an instance nobody is
//! looking at. The person's socket gets `-n -g <bundle> --args
//! -DRMCPHelperLaunch YES` and nothing else.

use std::ffi::OsString;
use std::fs::{DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::socket::{self, Reach};

/// Overrides the wait for a launched instance, in whole seconds.
pub const WAIT_ENV: &str = "DR_MCP_LAUNCH_WAIT";
/// Frame Job's wait: a cold launch measured about a second.
pub const DEFAULT_WAIT_SECS: u64 = 60;
/// The launch lock's name, beside the socket.
pub const LOCK_NAME: &str = "agent.launch.lock";
/// A scratch launch's stderr, beside the socket.
pub const LAUNCH_LOG_NAME: &str = "doctored-launch.log";
/// How every timeout sentence begins (`Session` recognises one by it).
pub const TIMEOUT_OPENING: &str = "Doctored did not answer at ";
/// How often the wait asks the socket.
const POLL: Duration = Duration::from_millis(50);
/// Appended with `YES` to every launch's arguments: the app's
/// `AppUpdater.helperLaunchArgument`, which defers its updater until a person
/// first activates the instance.
pub const HELPER_LAUNCH_ARGUMENT: &str = "-DRMCPHelperLaunch";

/// A positive whole number of seconds in `DR_MCP_LAUNCH_WAIT`, else 60 s.
pub fn launch_wait() -> Duration {
    let seconds = std::env::var(WAIT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_WAIT_SECS);
    Duration::from_secs(seconds)
}

/// Every `DR_*` variable and `ANTHROPIC_API_KEY` set in this process, sorted
/// by name: what a scratch launch passes on with `open --env`, so a test's
/// isolation (private folders, a dummy key, the working limit) reaches the
/// instance. `DR_AGENT_SOCKET` is among them only when set; `open_arguments`
/// passes the socket first and skips it here.
pub fn forwarded_environment() -> Vec<(OsString, OsString)> {
    let mut pairs: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| {
            name.to_str()
                .is_some_and(|name| name.starts_with("DR_") || name == "ANTHROPIC_API_KEY")
        })
        .collect();
    pairs.sort();
    pairs
}

/// The `/usr/bin/open` arguments that launch `bundle` for `socket`. Pure.
/// A scratch launch passes `DR_AGENT_SOCKET` first, then one `--env
/// NAME=VALUE` per `forwarded` pair in order (any `DR_AGENT_SOCKET` among
/// them skipped), and `-ApplePersistenceIgnoreState YES` after `--args`;
/// the person's socket gets `-n -g <bundle> --args -DRMCPHelperLaunch YES`
/// only. Both end with `HELPER_LAUNCH_ARGUMENT YES`: no person opened the
/// instance, so its updater waits for the first activation.
pub fn open_arguments(
    bundle: &Path,
    socket: &Path,
    scratch: bool,
    forwarded: &[(OsString, OsString)],
) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec!["-n".into(), "-g".into()];
    if scratch {
        let mut env = OsString::from(format!("{}=", socket::SOCKET_ENV));
        env.push(socket.as_os_str());
        let folder = socket.parent().unwrap_or(Path::new("/"));
        arguments.push("--env".into());
        arguments.push(env);
        for (name, value) in forwarded {
            if name == socket::SOCKET_ENV {
                continue;
            }
            let mut pair = name.clone();
            pair.push("=");
            pair.push(value);
            arguments.push("--env".into());
            arguments.push(pair);
        }
        arguments.push("--stderr".into());
        arguments.push(folder.join(LAUNCH_LOG_NAME).into_os_string());
    }
    arguments.push(bundle.as_os_str().to_os_string());
    arguments.push("--args".into());
    if scratch {
        arguments.push("-ApplePersistenceIgnoreState".into());
        arguments.push("YES".into());
    }
    arguments.push(HELPER_LAUNCH_ARGUMENT.into());
    arguments.push("YES".into());
    arguments
}

/// The sentence a launch that never bound answers.
pub fn timeout_sentence(socket: &Path, wait: Duration, bundle: &Path) -> String {
    format!(
        "{TIMEOUT_OPENING}{} within {} s of launching {}",
        socket.display(),
        wait.as_secs(),
        bundle.display()
    )
}

/// Whether the socket answered at once or after a launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launched {
    AlreadyUp,
    Started,
}

/// Makes sure something answers at `socket`, launching `bundle` if
/// nothing does once the lock is held. The error is a sentence; a launch
/// that never answered is `timeout_sentence`'s.
pub fn ensure_running(bundle: &Path, socket: &Path) -> Result<Launched, String> {
    let folder = socket
        .parent()
        .ok_or_else(|| format!("the socket path {} has no folder", socket.display()))?;
    if !folder.exists() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(folder)
            .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    }
    let lock_path = folder.join(LOCK_NAME);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|e| format!("could not open {}: {e}", lock_path.display()))?;
    lock.lock()
        .map_err(|e| format!("could not lock {}: {e}", lock_path.display()))?;
    match socket::reach(socket) {
        Reach::Answers => return Ok(Launched::AlreadyUp),
        Reach::Unreachable(reason) => return Err(reason),
        Reach::Absent => {}
    }
    let arguments = open_arguments(
        bundle,
        socket,
        socket::is_scratch(),
        &forwarded_environment(),
    );
    let opened = Command::new("/usr/bin/open")
        .args(&arguments)
        .output()
        .map_err(|e| format!("could not run /usr/bin/open: {e}"))?;
    if !opened.status.success() {
        return Err(format!(
            "/usr/bin/open {} failed ({}): {}",
            bundle.display(),
            opened.status,
            String::from_utf8_lossy(&opened.stderr).trim()
        ));
    }
    let wait = launch_wait();
    let started = Instant::now();
    loop {
        if socket::reach(socket) == Reach::Answers {
            drop(lock);
            return Ok(Launched::Started);
        }
        if started.elapsed() >= wait {
            drop(lock);
            return Err(timeout_sentence(socket, wait, bundle));
        }
        std::thread::sleep(POLL);
    }
}
