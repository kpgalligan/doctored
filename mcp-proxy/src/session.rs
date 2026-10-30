//! One client session: each message from stdin dispatched, each answer
//! written to stdout as one line under one lock, and the `list_changed`
//! check (MC-4).
//!
//! **What is answered here and what is forwarded.** `initialize` and `ping`
//! never touch the socket, so a client starts with no Doctored running and a
//! `ping` is answered while a long call holds the app. `tools/list` is the
//! app's when it answers within `LIST_READ_TIMEOUT` and the bundle's
//! snapshot otherwise; it never launches anything (MC-4). `tools/call`
//! launches its own bundle when no socket answers. Anything else is
//! forwarded when the app answers and is `-32601` when it does not.
//!
//! **`list_changed`.** After a list from the snapshot, or a launch, the
//! next forwarded call is followed by a live `tools/list`; when its tools
//! differ from the ones this session was last given — as JSON VALUES, never
//! bytes, since the catalog's key order differs between processes (a
//! trap `designs/agent-socket.md` records) — the client is told `notifications/tools/list_changed`.
//!
//! **A launch that never answered** strands the session: its later calls
//! that find no socket answer the stranded sentence and launch nothing, so
//! an instance that started but never bound is not joined by another on
//! every call. The check and the launch are one critical section per
//! session, so concurrent calls that all found no socket launch at most
//! once between them.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::bundle;
use crate::launch::{self, Launched};
use crate::mcp_versions;
use crate::socket::{self, Reach};

/// stdout, behind one lock: one message, one line, flushed.
pub struct Output {
    stdout: Mutex<std::io::Stdout>,
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl Output {
    pub fn new() -> Output {
        Output {
            stdout: Mutex::new(std::io::stdout()),
        }
    }

    /// Writes `message` as compact JSON and a newline, then flushes.
    pub fn write(&self, message: &Value) {
        let mut line = message.to_string().into_bytes();
        line.push(b'\n');
        let stdout = self.stdout.lock().unwrap_or_else(|e| e.into_inner());
        let mut handle = stdout.lock();
        let _ = handle.write_all(&line).and_then(|()| handle.flush());
    }
}

#[derive(Default)]
struct State {
    /// The tools this session's client was last given.
    last_given: Option<Value>,
    /// A live list is due after the next forwarded call.
    check_due: bool,
    /// The timeout sentence of this session's launch that never answered.
    stranded: Option<String>,
}

/// A client session's state, behind one lock.
pub struct Session {
    state: Mutex<State>,
    /// Held from a call's `stranded` check through its launch, so a call
    /// that waited behind a launch that never answered sees `stranded`
    /// rather than launching a second instance.
    launching: Mutex<()>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads one stdin line: a blank one is skipped; JSON that does not parse
/// is `-32700` and a batch `-32600`, each with a null id; a notification
/// (no `id`) or a response (no `method`) is dropped and never forwarded;
/// every other message is handled on a thread of its own.
pub fn dispatch(line: &[u8], session: &Arc<Session>, out: &Arc<Output>) {
    if line.iter().all(u8::is_ascii_whitespace) {
        return;
    }
    let message: Value = match serde_json::from_slice(line) {
        Ok(message) => message,
        Err(_) => return out.write(&error(Value::Null, -32700, "parse error")),
    };
    let Some(object) = message.as_object() else {
        let text = if message.is_array() {
            "batches are not supported"
        } else {
            "a message must be a JSON object"
        };
        return out.write(&error(Value::Null, -32600, text));
    };
    if !object.contains_key("id") || !object.contains_key("method") {
        return;
    }
    let (session, out) = (Arc::clone(session), Arc::clone(out));
    std::thread::spawn(move || session.handle(message, &out));
}

impl Session {
    pub fn new() -> Session {
        Session {
            state: Mutex::new(State::default()),
            launching: Mutex::new(()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Answers one request (it has an `id` and a `method`).
    pub fn handle(&self, message: Value, out: &Output) {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return out.write(&error(id, -32600, "the method must be a string"));
        };
        match method {
            "initialize" => out.write(&initialize(id, &message)),
            "ping" => out.write(&result(id, json!({}))),
            "tools/list" => out.write(&self.tools_list(id, &message)),
            "tools/call" => self.tools_call(id, &message, out),
            other => out.write(&forward_other(id, other, &message)),
        }
    }

    fn tools_list(&self, id: Value, message: &Value) -> Value {
        let path = match socket::socket_path() {
            Ok(path) => path,
            Err(reason) => return self.snapshot_list(id, &reason),
        };
        let answers = socket::reach(&path) == Reach::Answers;
        let why = if !answers {
            format!("nothing answers at {}", path.display())
        } else {
            match socket::post(&path, message, Some(socket::LIST_READ_TIMEOUT)) {
                Ok(Some(answer)) => match answer.pointer("/result/tools") {
                    Some(tools) if tools.is_array() => {
                        let mut state = self.state();
                        state.last_given = Some(tools.clone());
                        state.check_due = false;
                        return answer;
                    }
                    _ => format!("Doctored's tools/list at {} held no tools", path.display()),
                },
                Ok(None) => format!(
                    "Doctored answered tools/list at {} with no body",
                    path.display()
                ),
                Err(reason) => reason,
            }
        };
        self.snapshot_list(id, &why)
    }

    /// The snapshot's tools, or `-32603` naming the snapshot and `why` the
    /// app did not answer.
    fn snapshot_list(&self, id: Value, why: &str) -> Value {
        let snapshot = bundle::own_bundle().and_then(|bundle| bundle::read_snapshot(&bundle));
        match snapshot {
            Ok(snapshot) => {
                let mut state = self.state();
                state.last_given = Some(snapshot.tools.clone());
                state.check_due = true;
                result(id, json!({"tools": snapshot.tools}))
            }
            Err(reason) => error(
                id,
                -32603,
                &format!(
                    "no tools to list: {reason} ({}); and {why}",
                    bundle::SNAPSHOT_RELATIVE
                ),
            ),
        }
    }

    fn tools_call(&self, id: Value, message: &Value, out: &Output) {
        let path = match socket::socket_path() {
            Ok(path) => path,
            Err(reason) => return out.write(&call_error(id, &reason)),
        };
        match socket::reach(&path) {
            Reach::Answers => {}
            Reach::Unreachable(reason) => return out.write(&call_error(id, &reason)),
            Reach::Absent => {
                let _launching = self.launching.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(sentence) = self.state().stranded.clone() {
                    return out.write(&call_error(id, &sentence));
                }
                let bundle = match bundle::own_bundle() {
                    Ok(bundle) => bundle,
                    Err(reason) => return out.write(&call_error(id, &reason)),
                };
                match launch::ensure_running(&bundle, &path) {
                    Ok(Launched::Started) => self.state().check_due = true,
                    Ok(Launched::AlreadyUp) => {}
                    Err(reason) => {
                        if reason.starts_with(launch::TIMEOUT_OPENING) {
                            self.state().stranded = Some(stranded_sentence(&reason, &path));
                        }
                        eprintln!("doctored-mcp: {reason}");
                        return out.write(&call_error(id, &reason));
                    }
                }
            }
        }
        match socket::post(&path, message, None) {
            Ok(Some(answer)) => out.write(&answer),
            Ok(None) => {
                return out.write(&call_error(id, "Doctored answered the call with no body"))
            }
            Err(reason) => return out.write(&call_error(id, &reason)),
        }
        self.check_list(&path, out);
    }

    /// After a forwarded call: when a check is due, a live `tools/list`
    /// compared with the last-given tools as JSON values.
    fn check_list(&self, path: &Path, out: &Output) {
        if !self.state().check_due {
            return;
        }
        let ask =
            json!({"jsonrpc": "2.0", "id": "doctored-mcp-list-check", "method": "tools/list"});
        let live = match socket::post(path, &ask, None) {
            Ok(Some(answer)) => answer
                .pointer("/result/tools")
                .filter(|t| t.is_array())
                .cloned(),
            Ok(None) => None,
            Err(reason) => {
                eprintln!("doctored-mcp: the list check: {reason}");
                None
            }
        };
        let mut state = self.state();
        state.check_due = false;
        let Some(live) = live else { return };
        if state.last_given.as_ref() != Some(&live) {
            state.last_given = Some(live);
            out.write(&json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
        }
    }
}

fn initialize(id: Value, message: &Value) -> Value {
    let requested = message
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = bundle::own_bundle()
        .and_then(|bundle| bundle::read_snapshot(&bundle))
        .map(|snapshot| snapshot.version)
        .unwrap_or_else(|_| "0.0".to_string());
    result(
        id,
        json!({
            "protocolVersion": mcp_versions::negotiate(requested),
            "capabilities": {"tools": {"listChanged": true}},
            "serverInfo": {"name": "doctored", "version": version},
        }),
    )
}

/// Any method but the four above: the app's answer when it answers.
fn forward_other(id: Value, method: &str, message: &Value) -> Value {
    let Ok(path) = socket::socket_path() else {
        return error(id, -32601, &format!("method not found: {method}"));
    };
    if socket::reach(&path) != Reach::Answers {
        return error(id, -32601, &format!("method not found: {method}"));
    }
    match socket::post(&path, message, None) {
        Ok(Some(answer)) => answer,
        Ok(None) => error(id, -32603, "Doctored answered with no body"),
        Err(reason) => error(id, -32603, &reason),
    }
}

/// The sentence a stranded session's later calls answer.
fn stranded_sentence(timeout: &str, socket: &Path) -> String {
    let folder = socket.parent().unwrap_or(Path::new("/"));
    format!(
        "{timeout}; that instance may still be running, so this session will not launch \
         another — quit it and check {}",
        folder.display()
    )
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// A proxy-side failure of a `tools/call`, as the CallToolResult a client
/// shows the model.
fn call_error(id: Value, sentence: &str) -> Value {
    result(
        id,
        json!({"content": [{"type": "text", "text": sentence}], "isError": true}),
    )
}
