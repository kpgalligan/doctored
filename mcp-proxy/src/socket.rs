//! The agent server's socket, from the proxy's side: where it is (MC-1,
//! MC-6), whether something answers there, and one HTTP/1.1 POST over
//! it. The app's server is `tiny_http` 0.12 (`core/src/agent.rs`), which
//! answers a body of 32 768 bytes or more CHUNKED, and a `tools/list` is
//! about 240 KB, so the reader decodes both `Content-Length` and chunked
//! bodies (and reads to the close when a response has neither).

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

/// Overrides the socket path, for the app and the proxy alike (MC-6).
pub const SOCKET_ENV: &str = "DR_AGENT_SOCKET";

/// How long a forwarded `tools/list` may wait for the app before the
/// snapshot answers instead: the app serves one request at a time, so
/// another client's long call holds it.
pub const LIST_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest response body the reader takes (the app's own request cap).
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// `DR_AGENT_SOCKET` when set and not empty, else
/// `$HOME/Library/Application Support/Doctored/Agent/agent.sock` (MC-1).
pub fn socket_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(SOCKET_ENV).filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| "HOME is not set, so the agent socket's path is unknown".to_string())?;
    Ok(PathBuf::from(home).join("Library/Application Support/Doctored/Agent/agent.sock"))
}

/// True when `DR_AGENT_SOCKET` is set and not empty: a scratch instance,
/// which a launch tells where to bind (MC-6).
pub fn is_scratch() -> bool {
    std::env::var_os(SOCKET_ENV).is_some_and(|path| !path.is_empty())
}

/// What a connect to the socket found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Something accepted the connection.
    Answers,
    /// No file at the path, or a file nothing listens on (a quit's or a
    /// crash's leftover): Doctored is not running there.
    Absent,
    /// Anything else, in a sentence.
    Unreachable(String),
}

/// Connects to `path` and closes at once.
pub fn reach(path: &Path) -> Reach {
    match UnixStream::connect(path) {
        Ok(_) => Reach::Answers,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Reach::Absent
        }
        Err(error) => Reach::Unreachable(format!(
            "could not connect to Doctored at {}: {error}",
            path.display()
        )),
    }
}

/// POSTs `message` to `/mcp` over the socket and reads the answer: 200 →
/// its body as JSON, 202 → None (a notification's answer), anything else →
/// an error naming the status. `read_timeout` bounds each read (None waits
/// as long as the app takes); a read that times out is an error.
pub fn post(
    path: &Path,
    message: &Value,
    read_timeout: Option<Duration>,
) -> Result<Option<Value>, String> {
    let shown = path.display();
    let mut stream = UnixStream::connect(path)
        .map_err(|e| format!("could not connect to Doctored at {shown}: {e}"))?;
    stream
        .set_read_timeout(read_timeout)
        .map_err(|e| format!("could not set a read timeout on {shown}: {e}"))?;
    let body = serde_json::to_vec(message).map_err(|e| format!("could not encode: {e}"))?;
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("could not write to Doctored at {shown}: {e}"))?;
    let mut reader = BufReader::new(stream);
    let (status, body) = read_response(&mut reader).map_err(|e| match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => format!(
            "Doctored did not answer at {shown} within {} s",
            read_timeout.map_or(0, |t| t.as_secs())
        ),
        _ => format!("could not read Doctored's answer at {shown}: {e}"),
    })?;
    match status {
        200 => serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| format!("Doctored's answer at {shown} is not JSON: {e}")),
        202 => Ok(None),
        code => Err(format!("Doctored answered HTTP {code}")),
    }
}

fn invalid(what: &str) -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, what.to_string())
}

/// One header or chunk-size line, without its CRLF; EOF is an error.
fn read_line(reader: &mut impl BufRead) -> std::io::Result<String> {
    let mut line = Vec::new();
    if reader.read_until(b'\n', &mut line)? == 0 {
        return Err(invalid("the connection closed mid-response"));
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| invalid("a response line is not UTF-8"))
}

/// The status code and the decoded body of one HTTP/1.1 response.
fn read_response(reader: &mut impl BufRead) -> std::io::Result<(u16, Vec<u8>)> {
    let status_line = read_line(reader)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| invalid("the status line has no code"))?;
    let mut length = None;
    let mut chunked = false;
    loop {
        let line = read_line(reader)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid("a bad Content-Length"))?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
        }
    }
    let body = if chunked {
        read_chunked(reader)?
    } else if let Some(length) = length {
        if length > MAX_BODY_BYTES {
            return Err(invalid("the answer is over 8 MiB"));
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        body
    } else {
        let mut body = Vec::new();
        reader
            .take(MAX_BODY_BYTES as u64 + 1)
            .read_to_end(&mut body)?;
        body
    };
    if body.len() > MAX_BODY_BYTES {
        return Err(invalid("the answer is over 8 MiB"));
    }
    Ok((status, body))
}

/// A chunked body: hex sizes (extensions ignored), each chunk and its CRLF,
/// then the zero chunk and any trailer lines up to the blank one.
fn read_chunked(reader: &mut impl BufRead) -> std::io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = read_line(reader)?;
        let size = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| invalid("a bad chunk size"))?;
        if size == 0 {
            while !read_line(reader)?.is_empty() {}
            return Ok(body);
        }
        if body
            .len()
            .checked_add(size)
            .is_none_or(|end| end > MAX_BODY_BYTES)
        {
            return Err(invalid("the answer is over 8 MiB"));
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
        if &crlf != b"\r\n" {
            return Err(invalid("a chunk does not end in CRLF"));
        }
    }
}
