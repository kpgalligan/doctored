//! Integration tests for the embedded MCP server on its Unix socket: one
//! sequential scenario driven through the C FFI (`dr_agent_socket_*`) like
//! a host application would, with a raw `UnixStream` HTTP client standing
//! in for an MCP client, plus the claim policy's cases on the Rust API
//! (`doctored_core::agent`), each in its own temporary folder so they run
//! in parallel with the scenario and each other.

use std::ffi::{c_char, c_void, CStr, CString};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::agent::{self, AgentConfig, AgentServer, MAX_SOCKET_PATH_BYTES};
use doctored_core::ffi_agent::{
    dr_agent_socket_release, dr_agent_socket_start, dr_agent_string_create,
};
use serde_json::{json, Value};

/// Test handler: echoes the tool name and arguments back as a text block;
/// the tool name "fail" exercises the NULL (handler failure) path.
unsafe extern "C" fn echo_handler(
    _context: *mut c_void,
    tool_name: *const c_char,
    arguments_json: *const c_char,
) -> *mut c_char {
    let name = unsafe { CStr::from_ptr(tool_name) }.to_str().unwrap();
    let arguments = unsafe { CStr::from_ptr(arguments_json) }.to_str().unwrap();
    if name == "fail" {
        return ptr::null_mut();
    }
    let result = json!({
        "content": [{"type": "text", "text": format!("{name}:{arguments}")}],
        "isError": false,
    });
    let result = CString::new(result.to_string()).unwrap();
    unsafe { dr_agent_string_create(result.as_ptr()) }
}

const TOOLS: &str = r#"[
  {"name": "echo", "description": "Echoes.",
   "inputSchema": {"type": "object", "properties": {}}}
]"#;

/// Calls `dr_agent_socket_start` with `path` (None passes NULL) and
/// `tools`; Ok on true, else the `err_out` message (freed here).
fn ffi_start(path: Option<&str>, tools: &str) -> Result<(), String> {
    let path = path.map(|p| CString::new(p).unwrap());
    let name = CString::new("doctored-test").unwrap();
    let version = CString::new("0.0.0").unwrap();
    let tools = CString::new(tools).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let bound = unsafe {
        dr_agent_socket_start(
            path.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            name.as_ptr(),
            version.as_ptr(),
            tools.as_ptr(),
            echo_handler,
            ptr::null_mut(),
            &mut err,
        )
    };
    if bound {
        assert!(err.is_null());
        return Ok(());
    }
    assert!(!err.is_null(), "a refusal must set err_out");
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(err) };
    Err(message)
}

/// Sends one raw HTTP request over the socket and returns (status, body).
/// `head` is the request line plus headers, each ending in CRLF; this
/// appends `Content-Length` and `Connection: close`.
fn raw(socket: &Path, head: &str, body: &str) -> (u16, String) {
    let mut stream = UnixStream::connect(socket).unwrap();
    let request = format!(
        "{head}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status: u16 = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

/// Sends one HTTP request to `/mcp` and returns (status code, body).
fn http(socket: &Path, method: &str, body: &str) -> (u16, String) {
    let head = format!(
        "{method} /mcp HTTP/1.1\r\nHost: localhost\r\n\
         Content-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\n"
    );
    raw(socket, &head, body)
}

fn rpc(socket: &Path, message: Value) -> (u16, Value) {
    let (status, body) = http(socket, "POST", &message.to_string());
    let parsed = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// One test function: the FFI slot is process-global state (one listener
/// per process), so the protocol walk must be a single sequential scenario.
#[test]
fn mcp_session_over_the_unix_socket() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("Agent").join("agent.sock");
    let socket_text = socket.to_str().unwrap().to_string();

    // Refusals before anything binds: NULL, relative and over-long paths,
    // and a catalog that is not an array.
    assert!(ffi_start(None, TOOLS).unwrap_err().contains("socket_path"));
    assert!(ffi_start(Some("relative/agent.sock"), TOOLS)
        .unwrap_err()
        .contains("must be absolute"));
    let long = format!("/tmp/{}", "x".repeat(MAX_SOCKET_PATH_BYTES + 1 - 5));
    assert_eq!(long.len(), MAX_SOCKET_PATH_BYTES + 1);
    assert!(ffi_start(Some(&long), TOOLS)
        .unwrap_err()
        .contains("macOS allows 103"));
    assert!(ffi_start(Some(&socket_text), "{}")
        .unwrap_err()
        .contains("JSON array"));
    assert!(!socket.exists());

    ffi_start(Some(&socket_text), TOOLS).unwrap();
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "socket file mode {mode:o}");
    let folder_mode = std::fs::metadata(socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(folder_mode & 0o777, 0o700, "folder mode {folder_mode:o}");

    // initialize negotiates a known protocol version verbatim.
    let (status, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "2025-03-26",
                          "capabilities": {},
                          "clientInfo": {"name": "test", "version": "0"}}}),
    );
    assert_eq!(status, 200);
    assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(reply["result"]["serverInfo"]["name"], "doctored-test");
    assert!(reply["result"]["capabilities"]["tools"].is_object());

    // An unknown requested version is answered with the newest supported.
    let (_, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 2, "method": "initialize",
               "params": {"protocolVersion": "1999-01-01"}}),
    );
    assert_eq!(
        reply["result"]["protocolVersion"],
        doctored_core::mcp_versions::LATEST_VERSION
    );

    // Notifications get 202 with no body.
    let (status, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    assert_eq!(status, 202);
    assert_eq!(reply, Value::Null);

    // ping.
    let (status, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    );
    assert_eq!(status, 200);
    assert!(reply["result"].is_object());

    // tools/list returns the catalog verbatim.
    let (status, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}),
    );
    assert_eq!(status, 200);
    let expected: Value = serde_json::from_str(TOOLS).unwrap();
    assert_eq!(reply["result"]["tools"], expected);

    // tools/call round-trips through the handler.
    let (status, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
               "params": {"name": "echo", "arguments": {"x": 1}}}),
    );
    assert_eq!(status, 200);
    assert_eq!(reply["result"]["content"][0]["text"], r#"echo:{"x":1}"#);
    assert_eq!(reply["result"]["isError"], false);

    // A handler failure surfaces as a JSON-RPC internal error.
    let (_, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
               "params": {"name": "fail"}}),
    );
    assert_eq!(reply["error"]["code"], -32603);

    // Missing tool name is invalid params.
    let (_, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {}}),
    );
    assert_eq!(reply["error"]["code"], -32602);

    // Unknown method.
    let (_, reply) = rpc(
        &socket,
        json!({"jsonrpc": "2.0", "id": 8, "method": "resources/list"}),
    );
    assert_eq!(reply["error"]["code"], -32601);

    // Malformed JSON is a parse error over HTTP 400.
    let (status, body) = http(&socket, "POST", "{nope");
    assert_eq!(status, 400);
    assert!(body.contains("-32700"));

    // Batches are answered element-wise; notifications produce no entry.
    let (status, reply) = rpc(
        &socket,
        json!([
            {"jsonrpc": "2.0", "id": 9, "method": "ping"},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 10, "method": "ping"}
        ]),
    );
    assert_eq!(status, 200);
    assert_eq!(reply.as_array().map(Vec::len), Some(2));

    // GET (no server-push stream) and DELETE (no sessions) are 405.
    assert_eq!(http(&socket, "GET", "").0, 405);
    assert_eq!(http(&socket, "DELETE", "").0, 405);

    // Any other path is 404.
    let ping = json!({"jsonrpc": "2.0", "id": 11, "method": "ping"}).to_string();
    for path in ["/", "/mcp/x"] {
        let head = format!("POST {path} HTTP/1.1\r\nHost: localhost\r\n");
        assert_eq!(raw(&socket, &head, &ping).0, 404, "{path}");
    }

    // A second start is refused while this one is bound, at any path.
    let other = dir.path().join("Other").join("agent.sock");
    assert!(ffi_start(Some(other.to_str().unwrap()), TOOLS)
        .unwrap_err()
        .contains("already bound"));
    assert!(!other.exists());

    // Release unlinks the file it bound.
    dr_agent_socket_release();
    assert!(!socket.exists(), "release left {}", socket.display());
    // A release with none bound is safe.
    dr_agent_socket_release();
}

fn config(name: &str) -> AgentConfig {
    AgentConfig {
        name: name.to_string(),
        version: "0".to_string(),
        tools: json!([]),
        handler: Box::new(|_, _| None),
    }
}

/// `<fresh temp folder>/Agent/agent.sock`, with the folder handle that
/// keeps it alive.
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("Agent").join("agent.sock");
    (dir, socket)
}

/// The `serverInfo.name` the listener at `socket` answers `initialize` with.
fn answers(socket: &Path) -> String {
    let (status, reply) = rpc(
        socket,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    assert_eq!(status, 200);
    reply["result"]["serverInfo"]["name"]
        .as_str()
        .unwrap()
        .to_string()
}

fn start(socket: &Path, name: &str) -> Result<AgentServer, String> {
    agent::start_socket(socket, config(name))
}

#[test]
fn first_instance_binds_and_makes_its_folder_0700() {
    let (_dir, socket) = scratch();
    let server = start(&socket, "first").unwrap();
    let mode = std::fs::metadata(socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    assert_eq!(answers(&socket), "first");
    server.release();
    assert!(!socket.exists());
}

#[test]
fn a_live_listener_is_left_alone() {
    let (_dir, socket) = scratch();
    let first = start(&socket, "first").unwrap();
    let refusal = start(&socket, "second").err().unwrap();
    assert!(refusal.contains("another instance answers"), "{refusal}");
    assert_eq!(answers(&socket), "first");
    first.release();
}

#[test]
fn a_refused_leftover_is_replaced() {
    let (_dir, socket) = scratch();
    std::fs::create_dir(socket.parent().unwrap()).unwrap();
    std::fs::set_permissions(
        socket.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    // A std listener's drop leaves its file behind, refusing connections:
    // a crash's leftover.
    drop(UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    let server = start(&socket, "replacement").unwrap();
    assert_eq!(answers(&socket), "replacement");
    server.release();
}

#[test]
fn a_file_that_is_not_a_socket_is_left_alone() {
    let (_dir, socket) = scratch();
    std::fs::create_dir(socket.parent().unwrap()).unwrap();
    std::fs::set_permissions(
        socket.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::fs::write(&socket, b"keep me").unwrap();
    let refusal = start(&socket, "none").err().unwrap();
    assert!(refusal.contains("is not a socket"), "{refusal}");
    assert_eq!(std::fs::read(&socket).unwrap(), b"keep me");
}

#[test]
fn release_unlinks_only_its_own_file() {
    let (_dir, socket) = scratch();
    let a = start(&socket, "a").unwrap();
    std::fs::remove_file(&socket).unwrap();
    let b = start(&socket, "b").unwrap();
    a.release();
    assert!(socket.exists(), "A's release removed B's socket");
    assert_eq!(answers(&socket), "b");
    b.release();
    assert!(!socket.exists());
}

#[test]
fn a_shared_folder_is_refused_by_its_mode() {
    let (_dir, socket) = scratch();
    let folder = socket.parent().unwrap();
    std::fs::create_dir(folder).unwrap();
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let refusal = start(&socket, "none").err().unwrap();
    assert!(refusal.contains("is mode 0755"), "{refusal}");
    assert!(refusal.contains("(0700)"), "{refusal}");
    assert!(!socket.exists());
}

#[test]
fn a_symlinked_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("Real");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    let link = dir.path().join("Agent");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let refusal = start(&link.join("agent.sock"), "none").err().unwrap();
    assert!(refusal.contains("a symlink is refused"), "{refusal}");
    assert!(!real.join("agent.sock").exists());
}
