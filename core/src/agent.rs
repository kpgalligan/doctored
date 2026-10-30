//! Embedded MCP server: the streamable-HTTP transport and JSON-RPC layer
//! for the app's agent integration (tools only — no resources, prompts,
//! or server-initiated streams). The protocol lives here; executing a tool
//! is delegated to the host application through the handler installed at
//! start, so this module stays generic: it knows the catalog it was given
//! and nothing about documents.
//!
//! Transport notes: one listener, [`start_socket`], binds a Unix socket at
//! a path (the TCP listener and its Origin/Host/Content-Type
//! admission are gone), chmod 0600, in a folder that must be its owner's
//! alone. It answers JSON-RPC POSTs to `/mcp` (anything else is 404) with
//! a single `application/json` response (the streamable-HTTP spec allows
//! this in place of an SSE stream). Stateless — no session IDs are issued,
//! and GET/DELETE report 405 as the spec directs for servers without
//! server-push or client-terminated sessions. The socket file's folder
//! mode is the whole access control: only processes running as the
//! folder's owner can reach the socket, so no header check is needed.
//!
//! **Who binds** ([`prepare_socket_path`], the claim policy, moved here
//! from Frame Job's Swift so it is testable): the folder is made `0700`
//! and must then be a real folder owned by this user with no group or
//! world bits — a wrong mode is reported, never worked around. A socket at
//! the path that ACCEPTS a connection belongs to another instance and is
//! left alone; one that REFUSES is a crash's leftover and is unlinked;
//! anything that is not a socket is left alone too.
//!
//! **A socket listener never lets `tiny_http`'s `Drop` run against a file
//! that is no longer its own**: 0.12's `Server` drop connects to its path
//! and unlinks it unconditionally, so a listener whose file was unlinked
//! and re-bound by another instance would delete that instance's socket.
//! The listener records the file's device and inode when it binds
//! ([`SocketClaim`]), and [`AgentServer::release`] unlinks the file only
//! while it is still that one and abandons the listener without a join.
//! The serving thread holds its own `Arc` of the server, so dropping an
//! [`AgentServer`] never runs that `Drop` either: the listener serves
//! until process exit.

use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use serde_json::{json, Value};

/// Executes one tool call: (tool name, arguments JSON) -> CallToolResult
/// JSON (`{"content": [...], "isError": bool}`). None means the handler
/// itself failed (as opposed to the tool reporting an error result).
pub type ToolHandler = Box<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

pub struct AgentConfig {
    pub name: String,
    pub version: String,
    /// The `tools/list` catalog, pre-validated to be a JSON array.
    pub tools: Value,
    pub handler: ToolHandler,
}

/// macOS's `sun_path` holds 104 bytes with the terminating NUL, so a socket
/// path may be 103 bytes long (103 binds, 104 does not).
pub const MAX_SOCKET_PATH_BYTES: usize = 103;

/// The socket file a listener bound, by the device and inode `lstat` read
/// straight after the bind: what "still this listener's file" means.
pub struct SocketClaim {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl SocketClaim {
    /// True while `lstat(path)` answers the device and inode the bind left
    /// there. A file unlinked, or replaced by another listener's, is not.
    pub fn is_current(&self) -> bool {
        std::fs::symlink_metadata(&self.path)
            .map(|meta| meta.dev() == self.dev && meta.ino() == self.ino)
            .unwrap_or(false)
    }
}

pub struct AgentServer {
    /// Kept so the listener's ownership is explicit; the serving thread
    /// holds the other `Arc`, so this one dropping never runs the server's
    /// `Drop` (see the module doc).
    _server: Arc<tiny_http::Server>,
    _thread: JoinHandle<()>,
    /// The socket file this listener bound.
    claim: SocketClaim,
}

impl AgentServer {
    /// For quit: unlinks the socket file only if it is still the one this
    /// listener bound, then abandons the listener. Its thread is neither
    /// unblocked nor joined and nothing is dropped, so a call in flight
    /// cannot hold the caller; process exit ends the thread.
    pub fn release(self) {
        if self.claim.is_current() {
            let _ = std::fs::remove_file(&self.claim.path);
        }
        std::mem::forget(self);
    }
}

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// The one endpoint path; anything else is 404.
const MCP_PATH: &str = "/mcp";

/// The claim policy, run before a bind: Ok means the path is free to bind
/// (absent, or a refusing leftover this call unlinked); Err is the reason
/// nothing may bind, phrased for the host's one log line. In order:
/// - the path is absolute and at most [`MAX_SOCKET_PATH_BYTES`] long;
/// - its folder is made (`0700` at each level made) and must then be a real
///   folder (`lstat`, so a symlink is refused) owned by this user with no
///   group or world bits;
/// - an absent path is free; a file that is not a socket is left alone; a
///   socket that accepts a connection is another instance's; one that
///   refuses is unlinked; any other connect error refuses.
pub fn prepare_socket_path(path: &Path) -> Result<(), String> {
    let shown = path.display();
    if !path.is_absolute() {
        return Err(format!("socket path must be absolute: {shown}"));
    }
    let n = path.as_os_str().len();
    if n > MAX_SOCKET_PATH_BYTES {
        return Err(format!(
            "socket path is {n} bytes; macOS allows {MAX_SOCKET_PATH_BYTES}: {shown}"
        ));
    }
    let Some(folder) = path.parent() else {
        return Err(format!("socket path has no folder: {shown}"));
    };
    let folder_shown = folder.display();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .map_err(|e| format!("could not make the folder {folder_shown}: {e}"))?;
    let info = std::fs::symlink_metadata(folder)
        .map_err(|e| format!("could not read the folder {folder_shown}: {e}"))?;
    if !info.file_type().is_dir() {
        return Err(format!(
            "{folder_shown} is not a folder (a symlink is refused)"
        ));
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if info.uid() != uid {
        return Err(format!(
            "{folder_shown} is owned by uid {}, not this user ({uid})",
            info.uid()
        ));
    }
    let mode = info.mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "{folder_shown} is mode {:04o}; the socket's folder must be its owner's alone (0700)",
            mode & 0o7777
        ));
    }
    let existing = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("could not read {shown}: {e}")),
    };
    if !existing.file_type().is_socket() {
        return Err(format!(
            "{shown} exists and is not a socket; it is left alone"
        ));
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(format!(
            "another instance answers at {shown}; this one binds none"
        )),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("could not remove the leftover socket {shown}: {e}")),
            }
        }
        Err(e) => Err(format!("could not reach the socket at {shown}: {e}")),
    }
}

/// Binds the server on a Unix socket at `path` after
/// [`prepare_socket_path`] admits it, chmod 0600, and returns the running
/// server with its claim on the file. A catalog that is not a JSON array is
/// refused before anything at the path is touched.
pub fn start_socket(path: &Path, config: AgentConfig) -> Result<AgentServer, String> {
    if !config.tools.is_array() {
        return Err("tools catalog must be a JSON array".to_string());
    }
    prepare_socket_path(path)?;
    let shown = path.display();
    let server =
        tiny_http::Server::http_unix(path).map_err(|e| format!("could not bind {shown}: {e}"))?;
    // A fresh bind's file takes the umask (0755 under 022). On failure the
    // server drops here, which unlinks the file this call just made.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("could not make {shown} 0600: {e}"))?;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => {
            // Whatever is at the path now may not be this bind's file, so
            // the drop must not unlink it.
            std::mem::forget(server);
            return Err(format!("could not read {shown}: {e}"));
        }
    };
    let claim = SocketClaim {
        path: path.to_path_buf(),
        dev: meta.dev(),
        ino: meta.ino(),
    };
    let server = Arc::new(server);
    let accept = Arc::clone(&server);
    let thread = std::thread::spawn(move || serve(accept, config));
    Ok(AgentServer {
        _server: server,
        _thread: thread,
        claim,
    })
}

enum Reply {
    Json(u16, Value),
    Empty(u16),
}

fn serve(server: Arc<tiny_http::Server>, config: AgentConfig) {
    for mut request in server.incoming_requests() {
        // One request at a time: tool calls serialize through the host's
        // main thread anyway, and a panic must not take the loop down.
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_request(&mut request, &config)
        }))
        .unwrap_or_else(|_| Reply::Json(500, rpc_error(Value::Null, -32603, "internal error")));
        let _ = match reply {
            Reply::Json(status, body) => request.respond(
                tiny_http::Response::from_string(body.to_string())
                    .with_status_code(status)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .expect("static header"),
                    ),
            ),
            Reply::Empty(status) => {
                request.respond(tiny_http::Response::empty(tiny_http::StatusCode(status)))
            }
        };
    }
}

fn handle_request(request: &mut tiny_http::Request, config: &AgentConfig) -> Reply {
    let path = request.url().split('?').next().unwrap_or("");
    if path != MCP_PATH {
        return Reply::Json(
            404,
            rpc_error(
                Value::Null,
                -32600,
                "not found: the MCP endpoint is POST /mcp",
            ),
        );
    }
    match request.method() {
        tiny_http::Method::Post => {}
        // No server-initiated SSE stream (GET) and no sessions to delete.
        _ => return Reply::Empty(405),
    }
    if request
        .body_length()
        .is_some_and(|len| len > MAX_BODY_BYTES)
    {
        return Reply::Empty(413);
    }
    let mut body = Vec::new();
    let mut reader = request.as_reader().take(MAX_BODY_BYTES as u64 + 1);
    if reader.read_to_end(&mut body).is_err() || body.len() > MAX_BODY_BYTES {
        return Reply::Empty(413);
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Reply::Json(400, rpc_error(Value::Null, -32700, "parse error")),
    };
    // A batch (2025-03-26 revision) is answered element-wise; notifications
    // produce no entry. All-notification input gets the bodiless 202.
    let replies: Vec<Value> = match parsed {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|m| handle_message(m, config))
            .collect(),
        single => match handle_message(single, config) {
            Some(reply) => return Reply::Json(200, reply),
            None => return Reply::Empty(202),
        },
    };
    if replies.is_empty() {
        Reply::Empty(202)
    } else {
        Reply::Json(200, Value::Array(replies))
    }
}

/// Answers one JSON-RPC message; None for notifications (nothing to say).
fn handle_message(message: Value, config: &AgentConfig) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to a server request — we never send any, so ignore.
        return None;
    };
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(json!({}));
    let Some(id) = id else {
        return None; // notification: nothing is ever returned
    };

    let reply = match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let version = crate::mcp_versions::negotiate(requested);
            rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": config.name, "version": config.version},
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({"tools": config.tools})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "tools/call requires a tool name"));
            };
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let arguments = arguments.to_string();
            match (config.handler)(name, &arguments) {
                None => rpc_error(id, -32603, "tool handler failed"),
                Some(result) => match serde_json::from_str::<Value>(&result) {
                    Ok(value) if value.is_object() => rpc_result(id, value),
                    _ => rpc_error(id, -32603, "tool returned invalid JSON"),
                },
            }
        }
        _ => rpc_error(id, -32601, &format!("method not found: {method}")),
    };
    Some(reply)
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
