//! The built `doctored-mcp`, black-box, over its stdin and stdout.
//! Each test copies the binary into a fake bundle of its own,
//! `<tmp>/rzm-<pid>-<n>/Fake.app/Contents/Helpers/doctored-mcp`, beside a
//! snapshot, and points `DR_AGENT_SOCKET` into the same folder; a stub
//! server is `tiny_http`'s `Server::http_unix`, the app's own transport.
//!
//! **Two tests let a fake bundle reach `ensure_running`**, and only after
//! `Fixture::launchable` has given it an executable that records its launch,
//! its environment and its arguments and exits without binding, so
//! `/usr/bin/open` starts nothing that lasts. The recorded launch is the
//! only proof of the real `open -n -g --env … --args` path: no drive
//! launches the real app through the helper.
//! Every other `tools/call` from a fake bundle is made while a stub
//! answers, and the other call with no socket is the binary run outside any
//! bundle, which is refused before a launch. Every stdout line of every
//! proxy must parse as JSON-RPC 2.0; the reader thread fails the test on
//! the first one that does not.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use doctored_mcp::launch::{open_arguments, LAUNCH_LOG_NAME};
use doctored_mcp::mcp_versions::LATEST_VERSION;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A folder of the test's own with a fake bundle and a socket path in it.
struct Fixture {
    dir: PathBuf,
    exe: PathBuf,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture::with_tools(snapshot_tools())
    }

    fn with_tools(tools: Value) -> Fixture {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("rzm-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let helpers = dir.join("Fake.app/Contents/Helpers");
        let resources = dir.join("Fake.app/Contents/Resources");
        std::fs::create_dir_all(&helpers).expect("the fake bundle is made");
        std::fs::create_dir_all(&resources).expect("the fake bundle is made");
        let exe = helpers.join("doctored-mcp");
        std::fs::copy(env!("CARGO_BIN_EXE_doctored-mcp"), &exe).expect("the binary is copied");
        let snapshot = json!({"version": "7.7.7", "tools": tools});
        std::fs::write(resources.join("agent-catalog.json"), snapshot.to_string())
            .expect("the snapshot is written");
        let socket = dir.join("agent.sock");
        assert!(
            socket.as_os_str().len() <= 103,
            "{} is too long",
            socket.display()
        );
        Fixture { dir, exe, socket }
    }

    fn proxy(&self) -> Proxy {
        Proxy::start(&self.exe, &self.socket, &[])
    }

    /// Where a launched fake writes its environment (`env`) and then one
    /// line per argument.
    fn launch_record(&self) -> PathBuf {
        self.dir.join("launch-record")
    }

    /// Makes the fake bundle launchable: an `Info.plist` and a script that
    /// appends a line to the returned file, writes `launch_record`, and
    /// exits, never binding.
    fn launchable(&self) -> PathBuf {
        let contents = self.dir.join("Fake.app/Contents");
        let launches = self.dir.join("launches");
        std::fs::create_dir_all(contents.join("MacOS")).expect("the fake bundle is made");
        std::fs::write(
            contents.join("Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\"><dict>\n\
             <key>CFBundleExecutable</key><string>fake</string>\n\
             <key>CFBundleIdentifier</key><string>test.doctored-mcp.fake</string>\n\
             <key>CFBundlePackageType</key><string>APPL</string>\n\
             </dict></plist>\n",
        )
        .expect("the Info.plist is written");
        let script = contents.join("MacOS/fake");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho launched >> '{}'\nenv > '{record}'\n\
                 for a in \"$@\"; do echo \"$a\" >> '{record}'; done\n",
                launches.display(),
                record = self.launch_record().display()
            ),
        )
        .expect("the script is written");
        let mut mode = std::fs::metadata(&script)
            .expect("the script")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        std::fs::set_permissions(&script, mode).expect("the script is executable");
        launches
    }
}

/// LaunchServices' own registrar: `/usr/bin/open` registers the bundle it
/// launches, so a launchable fixture unregisters it before its folder goes.
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Versions/A/\
                          Frameworks/LaunchServices.framework/Versions/A/Support/lsregister";

impl Drop for Fixture {
    fn drop(&mut self) {
        let bundle = self.dir.join("Fake.app");
        if bundle.join("Contents/Info.plist").exists() {
            let _ = Command::new(LSREGISTER).arg("-u").arg(&bundle).status();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn snapshot_tools() -> Value {
    json!([
        {"name": "get_project", "description": "the project",
         "inputSchema": {"type": "object", "properties": {"project": {"type": "string"}}}},
        {"name": "add_marker", "description": "a marker",
         "inputSchema": {"type": "object", "properties": {"at": {"type": "integer"},
                                                         "name": {"type": "string"}}}}
    ])
}

/// How long a proxy may take from its spawn to its answer to the readiness
/// ping `Proxy::start` sends, so no later bound times the exec of the copied
/// binary (0.65–1.30 s for one exec under `make test`, measured in B75).
const START_BOUND: Duration = Duration::from_secs(60);

/// The host's power mode and load, read only, for a timeout's message: the
/// `lowpowermode` and `powermode` lines of `pmset -g` and `vm.loadavg`.
fn host_state() -> String {
    let read = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default()
    };
    let pmset = read("/usr/bin/pmset", &["-g"]);
    let mut parts: Vec<String> = pmset
        .lines()
        // "powermode" also matches "lowpowermode".
        .filter(|line| line.to_ascii_lowercase().contains("powermode"))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if parts.is_empty() {
        parts.push("pmset -g: no power mode line".to_string());
    }
    let load = read("/usr/sbin/sysctl", &["-n", "vm.loadavg"]);
    parts.push(format!("vm.loadavg {}", load.trim()));
    format!("host: {}", parts.join("; "))
}

/// A proxy child; its stdout lines arrive on a channel, each already
/// checked to be JSON-RPC 2.0. `start` returns once the proxy has answered
/// a readiness ping.
struct Proxy {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Result<Value, String>>,
}

impl Proxy {
    /// `extra` adds environment variables to the proxy's own.
    fn start(exe: &Path, socket: &Path, extra: &[(&str, &str)]) -> Proxy {
        let mut child = Command::new(exe)
            .env("DR_AGENT_SOCKET", socket)
            .env("DR_MCP_LAUNCH_WAIT", "1")
            .envs(extra.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the proxy starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let checked = match serde_json::from_str::<Value>(&line) {
                    Ok(value) if value.get("jsonrpc") == Some(&json!("2.0")) => Ok(value),
                    _ => Err(line),
                };
                if send.send(checked).is_err() {
                    break;
                }
            }
        });
        let mut proxy = Proxy {
            child,
            stdin,
            lines,
        };
        proxy.send(json!({"jsonrpc": "2.0", "id": "ready", "method": "ping"}));
        assert_eq!(
            proxy.next(START_BOUND),
            json!({"jsonrpc": "2.0", "id": "ready", "result": {}}),
            "the readiness ping"
        );
        proxy
    }

    fn send_raw(&mut self, text: &str) {
        self.stdin.write_all(text.as_bytes()).expect("stdin write");
        self.stdin.write_all(b"\n").expect("stdin write");
        self.stdin.flush().expect("stdin flush");
    }

    fn send(&mut self, message: Value) {
        self.send_raw(&message.to_string());
    }

    fn request(&mut self, id: i64, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    /// The next stdout line within `within`; a timeout or a line that is
    /// not JSON-RPC fails the test.
    fn next(&self, within: Duration) -> Value {
        match self.lines.recv_timeout(within) {
            Ok(Ok(value)) => value,
            Ok(Err(line)) => panic!("a stdout line is not JSON-RPC 2.0: {line:?}"),
            Err(RecvTimeoutError::Timeout) => {
                panic!("no answer within {within:?} ({})", host_state())
            }
            Err(RecvTimeoutError::Disconnected) => panic!("the proxy's stdout closed"),
        }
    }

    /// Nothing on stdout for `span`.
    fn quiet(&self, span: Duration) {
        match self.lines.recv_timeout(span) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(line) => panic!("expected no output, got {line:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("the proxy's stdout closed"),
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type Handler = Box<dyn Fn(&Value) -> Value + Send + Sync>;

/// A stub agent server on the socket: one request at a time, as the app's
/// is, each answered 200 with what `handler` returns for its body.
struct Stub {
    server: Arc<tiny_http::Server>,
}

impl Stub {
    fn start(path: &Path, handler: Handler) -> Stub {
        let server = Arc::new(tiny_http::Server::http_unix(path).expect("the stub binds"));
        let serving = Arc::clone(&server);
        std::thread::spawn(move || {
            for mut request in serving.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let message: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let answer = handler(&message);
                let text = match answer.get("raw") {
                    Some(Value::String(raw)) => raw.clone(),
                    _ => answer.to_string(),
                };
                let _ = request.respond(
                    tiny_http::Response::from_string(text).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .expect("static header"),
                    ),
                );
            }
        });
        Stub { server }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

fn answer(message: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": message.get("id").cloned().unwrap_or(Value::Null),
           "result": result})
}

/// A stub that lists `tools` and answers every call with a text result,
/// after `hold` for a call.
fn lister(tools: Value, hold: Duration) -> Handler {
    Box::new(
        move |message| match message.get("method").and_then(Value::as_str) {
            Some("tools/list") => answer(message, json!({"tools": tools})),
            Some("tools/call") => {
                std::thread::sleep(hold);
                answer(
                    message,
                    json!({"content": [{"type": "text", "text": "from the stub"}], "isError": false}),
                )
            }
            _ => json!({"jsonrpc": "2.0", "id": message.get("id"),
                    "error": {"code": -32601, "message": "stub"}}),
        },
    )
}

/// As `lister`, but a call sends `()` on `arrived` BEFORE it sleeps `hold`,
/// so a test knows the call is held in the app rather than guessing.
fn holder(tools: Value, hold: Duration, arrived: std::sync::mpsc::Sender<()>) -> Handler {
    let arrived = std::sync::Mutex::new(arrived);
    Box::new(
        move |message| match message.get("method").and_then(Value::as_str) {
            Some("tools/list") => answer(message, json!({"tools": tools})),
            Some("tools/call") => {
                let _ = arrived.lock().expect("the sender").send(());
                std::thread::sleep(hold);
                answer(
                    message,
                    json!({"content": [{"type": "text", "text": "from the stub"}], "isError": false}),
                )
            }
            _ => json!({"jsonrpc": "2.0", "id": message.get("id"),
                    "error": {"code": -32601, "message": "stub"}}),
        },
    )
}

/// `value` as JSON text with every object's keys in REVERSE sorted order.
fn reversed(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.reverse();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::String(key.clone()), reversed(&map[key])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(reversed).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

fn call_params() -> Value {
    json!({"name": "get_project", "arguments": {}})
}

#[test]
fn initialize_negotiates_and_reports_the_snapshot_version() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(1, "initialize", json!({"protocolVersion": "2025-03-26"}));
    let known = proxy.next(Duration::from_secs(5));
    assert_eq!(known["id"], json!(1));
    assert_eq!(known["result"]["protocolVersion"], json!("2025-03-26"));
    assert_eq!(
        known["result"]["capabilities"]["tools"]["listChanged"],
        json!(true)
    );
    assert_eq!(
        known["result"]["serverInfo"],
        json!({"name": "doctored", "version": "7.7.7"})
    );
    proxy.request(2, "initialize", json!({"protocolVersion": "1999-01-01"}));
    let unknown = proxy.next(Duration::from_secs(5));
    assert_eq!(unknown["id"], json!(2));
    assert_eq!(unknown["result"]["protocolVersion"], json!(LATEST_VERSION));
}

#[test]
fn ping_is_answered_with_an_empty_result() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(5, "ping", json!({}));
    assert_eq!(
        proxy.next(Duration::from_secs(5)),
        json!({"jsonrpc": "2.0", "id": 5, "result": {}})
    );
}

#[test]
fn tools_list_with_no_socket_is_the_snapshot() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(3, "tools/list", json!({}));
    let listed = proxy.next(Duration::from_secs(5));
    assert_eq!(listed["id"], json!(3));
    assert_eq!(listed["result"]["tools"], snapshot_tools());
    assert!(!fixture.socket.exists(), "nothing made a socket");
}

#[test]
fn tools_list_and_tools_call_are_forwarded_and_a_chunked_list_is_read() {
    let fixture = Fixture::new();
    let big: Vec<Value> = (0..400)
        .map(|i| {
            json!({"name": format!("tool_{i}"), "description": "x".repeat(200),
                        "inputSchema": {"type": "object"}})
        })
        .collect();
    let big = Value::Array(big);
    assert!(
        big.to_string().len() > 32 * 1024,
        "the list is chunked by tiny_http"
    );
    let _stub = Stub::start(&fixture.socket, lister(big.clone(), Duration::ZERO));
    let mut proxy = fixture.proxy();
    proxy.request(1, "tools/list", json!({}));
    let listed = proxy.next(Duration::from_secs(10));
    assert_eq!(listed["id"], json!(1));
    assert_eq!(listed["result"]["tools"], big);
    proxy.request(2, "tools/call", call_params());
    let called = proxy.next(Duration::from_secs(10));
    assert_eq!(called["id"], json!(2));
    assert_eq!(
        called["result"]["content"][0]["text"],
        json!("from the stub")
    );
    assert_eq!(called["result"]["isError"], json!(false));
    // The list was the app's, so no check was due and nothing follows.
    proxy.quiet(Duration::from_secs(1));
}

#[test]
fn list_changed_follows_a_call_when_the_live_list_differs_from_the_snapshot() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(1, "tools/list", json!({}));
    assert_eq!(
        proxy.next(Duration::from_secs(5))["result"]["tools"],
        snapshot_tools()
    );
    let mut newer = snapshot_tools();
    newer.as_array_mut().expect("an array").push(
        json!({"name": "probe_new", "description": "new", "inputSchema": {"type": "object"}}),
    );
    let _stub = Stub::start(&fixture.socket, lister(newer, Duration::ZERO));
    proxy.request(2, "tools/call", call_params());
    let called = proxy.next(Duration::from_secs(10));
    assert_eq!(called["id"], json!(2));
    assert_eq!(
        called["result"]["content"][0]["text"],
        json!("from the stub")
    );
    let notified = proxy.next(Duration::from_secs(5));
    assert_eq!(
        notified,
        json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
    );
}

#[test]
fn no_list_changed_when_the_live_list_differs_only_in_key_order() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(1, "tools/list", json!({}));
    assert_eq!(
        proxy.next(Duration::from_secs(5))["result"]["tools"],
        snapshot_tools()
    );
    let raw = format!(
        "{{\"result\":{{\"tools\":{}}},\"jsonrpc\":\"2.0\",\"id\":\"doctored-mcp-list-check\"}}",
        reversed(&snapshot_tools())
    );
    let base = lister(snapshot_tools(), Duration::ZERO);
    let handler: Handler = Box::new(move |message| {
        if message.get("method") == Some(&json!("tools/list")) {
            json!({"raw": raw})
        } else {
            base(message)
        }
    });
    let _stub = Stub::start(&fixture.socket, handler);
    proxy.request(2, "tools/call", call_params());
    assert_eq!(proxy.next(Duration::from_secs(10))["id"], json!(2));
    proxy.quiet(Duration::from_secs(2));
}

/// How long a ping may take while a call is held. Measured in B75:
/// the old test slept 300 ms after spawning the copied binary and then
/// timed the ping, so the clock started before the proxy read stdin; under
/// `make test` its first exec took 0.65–1.30 s and the ping 340–990 ms
/// (alone: 0.31 s and under 1 ms). This test waits for `initialize`'s
/// answer and for the call to reach the stub instead, and its ping took
/// 0.07–1.3 ms over three `make test` runs and three runs alone, so 1 s
/// stays, with the margin for a loaded host.
const PING_BOUND: Duration = Duration::from_secs(1);
/// How long the stub holds the call; it must stay above `PING_BOUND` plus
/// 2 s, so the ping's answer can only come first if it is answered on its
/// own thread.
const CALL_HOLD: Duration = Duration::from_secs(3);

#[test]
fn ping_is_answered_while_a_call_is_held() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(0, "initialize", json!({"protocolVersion": "2025-03-26"}));
    assert_eq!(proxy.next(Duration::from_secs(10))["id"], json!(0));
    let (tx, rx) = channel();
    let _stub = Stub::start(&fixture.socket, holder(snapshot_tools(), CALL_HOLD, tx));
    proxy.request(1, "tools/call", call_params());
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the call reaches the stub");
    let sent = Instant::now();
    proxy.request(2, "ping", json!({}));
    let first = proxy.next(PING_BOUND);
    assert_eq!(
        first["id"],
        json!(2),
        "the ping is answered before the call"
    );
    assert!(sent.elapsed() < PING_BOUND);
    let second = proxy.next(Duration::from_secs(10));
    assert_eq!(second["id"], json!(1));
}

#[test]
fn tools_list_answers_the_snapshot_while_another_client_holds_the_app() {
    let fixture = Fixture::new();
    let mut live = snapshot_tools();
    live.as_array_mut()
        .expect("an array")
        .push(json!({"name": "live_only", "inputSchema": {"type": "object"}}));
    let _stub = Stub::start(&fixture.socket, lister(live, Duration::from_secs(10)));
    let other = fixture.socket.clone();
    std::thread::spawn(move || {
        let body = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                          "params": {"name": "get_project", "arguments": {}}})
        .to_string();
        if let Ok(mut stream) = UnixStream::connect(&other) {
            let _ = write!(
                stream,
                "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink);
        }
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut proxy = fixture.proxy();
    let sent = Instant::now();
    proxy.request(1, "tools/list", json!({}));
    let listed = proxy.next(Duration::from_secs(8));
    assert!(sent.elapsed() < Duration::from_secs(8));
    assert_eq!(listed["id"], json!(1));
    assert_eq!(listed["result"]["tools"], snapshot_tools());
}

#[test]
fn an_unknown_method_with_no_socket_is_method_not_found() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.request(4, "resources/list", json!({}));
    let answered = proxy.next(Duration::from_secs(5));
    assert_eq!(answered["id"], json!(4));
    assert_eq!(answered["error"]["code"], json!(-32601));
    assert_eq!(
        answered["error"]["message"],
        json!("method not found: resources/list")
    );
}

#[test]
fn unparsable_input_and_a_batch_are_refused_with_a_null_id() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.send_raw("this is not json");
    let parse = proxy.next(Duration::from_secs(5));
    assert_eq!(parse["id"], Value::Null);
    assert_eq!(parse["error"]["code"], json!(-32700));
    proxy.send_raw(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#);
    let batch = proxy.next(Duration::from_secs(5));
    assert_eq!(batch["id"], Value::Null);
    assert_eq!(batch["error"]["code"], json!(-32600));
    assert_eq!(
        batch["error"]["message"],
        json!("batches are not supported")
    );
}

#[test]
fn a_notification_and_a_response_produce_no_output() {
    let fixture = Fixture::new();
    let mut proxy = fixture.proxy();
    proxy.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    proxy.send(json!({"jsonrpc": "2.0", "id": 77, "result": {}}));
    proxy.send_raw("");
    proxy.quiet(Duration::from_millis(500));
    proxy.request(6, "ping", json!({}));
    assert_eq!(proxy.next(Duration::from_secs(5))["id"], json!(6));
}

#[test]
fn concurrent_calls_in_one_session_launch_at_most_once_when_none_answers() {
    let fixture = Fixture::new();
    let launches = fixture.launchable();
    let mut proxy = fixture.proxy();
    proxy.request(1, "tools/call", call_params());
    proxy.request(2, "tools/call", call_params());
    let mut texts: Vec<String> = (0..2)
        .map(|_| {
            let called = proxy.next(Duration::from_secs(15));
            assert_eq!(called["result"]["isError"], json!(true), "{called}");
            called["result"]["content"][0]["text"]
                .as_str()
                .expect("a text")
                .to_string()
        })
        .collect();
    texts.sort_by_key(|text| text.contains("will not launch another"));
    assert!(
        texts[0].starts_with("Doctored did not answer at "),
        "{texts:?}"
    );
    assert!(!texts[0].contains("will not launch another"), "{texts:?}");
    assert!(texts[1].contains("will not launch another"), "{texts:?}");
    // A launch's line is written by the launched script; give a second
    // launch, if any, time to write its own.
    let deadline = Instant::now() + Duration::from_secs(3);
    let count = || {
        std::fs::read_to_string(&launches)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    while Instant::now() < deadline && count() < 2 {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(count(), 1, "launches: {}", count());
}

#[test]
fn outside_a_bundle_a_call_is_an_error_naming_the_executable() {
    let fixture = Fixture::new();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_doctored-mcp"))
        .canonicalize()
        .expect("the binary resolves");
    let mut proxy = Proxy::start(&exe, &fixture.socket, &[]);
    proxy.request(1, "tools/call", call_params());
    let called = proxy.next(Duration::from_secs(5));
    assert_eq!(called["id"], json!(1));
    assert_eq!(called["result"]["isError"], json!(true));
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("a text");
    assert!(text.contains("not inside a Doctored bundle"), "{text}");
    assert!(text.contains(&exe.display().to_string()), "{text}");
    proxy.request(2, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(
        proxy.next(Duration::from_secs(5))["result"]["serverInfo"]["version"],
        json!("0.0")
    );
}

#[test]
fn open_arguments_for_a_scratch_socket_and_the_persons() {
    let bundle = Path::new("/tmp/x/Doctored.app");
    let socket = Path::new("/tmp/s/agent.sock");
    let forwarded: Vec<(OsString, OsString)> = vec![
        ("ANTHROPIC_API_KEY".into(), "dummy".into()),
        ("DR_ACTIONS_DIR".into(), "/tmp/s/actions".into()),
    ];
    let scratch: Vec<String> = open_arguments(bundle, socket, true, &forwarded)
        .into_iter()
        .map(|a| a.into_string().expect("UTF-8"))
        .collect();
    let log = format!("/tmp/s/{LAUNCH_LOG_NAME}");
    assert_eq!(
        scratch,
        [
            "-n",
            "-g",
            "--env",
            "DR_AGENT_SOCKET=/tmp/s/agent.sock",
            "--env",
            "ANTHROPIC_API_KEY=dummy",
            "--env",
            "DR_ACTIONS_DIR=/tmp/s/actions",
            "--stderr",
            log.as_str(),
            "/tmp/x/Doctored.app",
            "--args",
            "-ApplePersistenceIgnoreState",
            "YES"
        ]
    );
    let persons: Vec<String> = open_arguments(bundle, socket, false, &forwarded)
        .into_iter()
        .map(|a| a.into_string().expect("UTF-8"))
        .collect();
    assert_eq!(persons, ["-n", "-g", "/tmp/x/Doctored.app"]);
}

#[test]
fn a_scratch_launch_forwards_the_environment_and_arguments() {
    let fixture = Fixture::new();
    let launches = fixture.launchable();
    let mut proxy = Proxy::start(
        &fixture.exe,
        &fixture.socket,
        &[
            ("DR_PROBE_MARK", "forwarded"),
            ("ANTHROPIC_API_KEY", "dummy"),
        ],
    );
    proxy.request(1, "tools/call", call_params());
    let called = proxy.next(Duration::from_secs(15));
    assert_eq!(called["result"]["isError"], json!(true), "{called}");
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("a text");
    assert!(text.starts_with("Doctored did not answer at "), "{text}");
    let socket_line = format!("DR_AGENT_SOCKET={}", fixture.socket.display());
    let expected = [
        socket_line.as_str(),
        "DR_PROBE_MARK=forwarded",
        "ANTHROPIC_API_KEY=dummy",
        "-ApplePersistenceIgnoreState",
        "YES",
    ];
    let record = fixture.launch_record();
    let holds_all = || {
        let text = std::fs::read_to_string(&record).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        expected.iter().all(|line| lines.contains(line))
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !holds_all() {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        holds_all(),
        "launch-record: {:?}",
        std::fs::read_to_string(&record).unwrap_or_default()
    );
    let count = std::fs::read_to_string(&launches)
        .map(|text| text.lines().count())
        .unwrap_or(0);
    assert_eq!(count, 1, "launches: {count}");
}
