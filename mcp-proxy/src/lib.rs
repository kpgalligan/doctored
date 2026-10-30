#![forbid(unsafe_code)]
//! `doctored_mcp` — the stdio MCP server bundled as
//! `Doctored.app/Contents/Helpers/doctored-mcp` (decisions MC-3, MC-4,
//! MC-6 in `designs/agent-socket.md`). An MCP client starts it; it speaks newline-delimited
//! JSON-RPC on stdin and stdout and reaches the running app over the agent
//! server's Unix socket. It answers `initialize` and `ping` itself, answers
//! `tools/list` from the app when one answers and from its bundle's catalog
//! snapshot when none does, and launches its own bundle in the background
//! on the first `tools/call` that finds no socket.
//!
//! stdout carries protocol only; anything else this process says goes to
//! stderr, each line starting `doctored-mcp: ` (never `[AgentServer] `, the app's
//! parity check's prefix). The module map is `mcp-proxy/CLAUDE.md`.

pub mod bundle;
pub mod launch;
#[path = "../../core/src/mcp_versions.rs"]
pub mod mcp_versions;
pub mod session;
pub mod socket;

use std::io::BufRead;
use std::sync::Arc;

/// The stdin loop: one `Session` and one `Output` for the process, each
/// line dispatched as it arrives (a request on its own thread, so a `ping`
/// is answered while a long call is held). At stdin's end the process exits
/// at once, without waiting for a call still in flight: the client that
/// would read its answer has gone.
pub fn run() -> ! {
    let session = Arc::new(session::Session::new());
    let out = Arc::new(session::Output::new());
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        match input.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => session::dispatch(&line, &session, &out),
            Err(error) => {
                eprintln!("doctored-mcp: stdin: {error}");
                break;
            }
        }
    }
    std::process::exit(0)
}
