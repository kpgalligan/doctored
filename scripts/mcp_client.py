"""The one MCP client every `scripts/` drive and launcher shares.

The app's agent server listens only on a Unix socket: `DR_AGENT_SOCKET` when
set, else `~/Library/Application Support/Doctored/Agent/agent.sock`, which no
test may bind. So every owned instance gets its own socket from
`scratch_socket()` (a fresh `0700` folder under the temp dir, because macOS's
`sun_path` holds 103 bytes), the launcher sets `DR_AGENT_SOCKET` to it, and
`Client(socket_path)` speaks MCP's streamable HTTP over it.

`Client.post` uses `http.client` on AF_UNIX, which decodes the chunked bodies
`tiny_http` sends for any answer of 32 KB or more (a `tools/list` is about
600 KB). `rpc`, `call` and `open` keep the semantics `verify_phase1.Client`
had over TCP. `socket_pid` is read only and for diagnostics: no drive picks a
process to signal by it.

Standard library only; imports nothing from `scripts/`, so a probe runner or a
measure script in a subfolder can import it with nothing else on its path.
"""

import http.client
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

MAX_SOCKET_PATH_BYTES = 103


class _UnixHTTPConnection(http.client.HTTPConnection):
    """`http.client` over AF_UNIX: the host is only the Host header."""

    def __init__(self, socket_path, timeout):
        super().__init__("localhost", timeout=timeout)
        self.socket_path = socket_path

    def connect(self):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(self.timeout)
        try:
            sock.connect(self.socket_path)
        except BaseException:
            sock.close()
            raise
        self.sock = sock


class Client:
    """An owned instance's MCP endpoint, at its socket. `timeout` is each
    request's ceiling in seconds; a subclass that only sets it is how a drive
    with long calls waits longer."""

    timeout = 30

    def __init__(self, socket_path, timeout=None):
        if isinstance(socket_path, int):
            raise TypeError(f"Client takes the instance's socket path, not the port {socket_path}: "
                            "the agent server has no TCP listener")
        self.socket_path = str(socket_path)
        if timeout is not None:
            self.timeout = timeout
        self.serial = 0

    def post(self, message, timeout=None):
        """POSTs one JSON-RPC message (or batch) to `/mcp`: its JSON answer,
        or None for a bodiless one (202). Raises RuntimeError on HTTP >= 400
        and OSError when nothing answers at the socket."""
        connection = _UnixHTTPConnection(self.socket_path, timeout or self.timeout)
        try:
            connection.request("POST", "/mcp", body=json.dumps(message).encode(),
                               headers={"Content-Type": "application/json",
                                        "Accept": "application/json, text/event-stream"})
            response = connection.getresponse()
            raw = response.read()
            status = response.status
        finally:
            connection.close()
        if status >= 400:
            raise RuntimeError(f"HTTP {status} at {self.socket_path}: "
                               f"{raw[:300].decode(errors='replace')}")
        if not raw.strip():
            return None
        return json.loads(raw)

    def rpc(self, method, params):
        self.serial += 1
        reply = self.post({"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params})
        assert reply is not None, (method, "no answer")
        assert "error" not in reply, reply
        return reply["result"]

    def call(self, tool_name, fail=False, **arguments):
        result = self.rpc("tools/call", {"name": tool_name, "arguments": arguments})
        assert bool(result.get("isError")) == fail, (tool_name, result)
        text = next((c["text"] for c in result.get("content", []) if c["type"] == "text"), None)
        if fail:
            return text or ""
        if text:
            try:
                return json.loads(text)
            except json.JSONDecodeError:
                pass
        return result

    def open(self, path):
        result = self.call("open_document", path=str(path))
        return result["document"]["id"]


def scratch_socket(prefix="rz-"):
    """`<a fresh 0700 folder under the temp dir>/agent.sock`. Raises
    ValueError (removing the folder) when the path passes 103 bytes."""
    folder = tempfile.mkdtemp(prefix=prefix, dir=tempfile.gettempdir())
    os.chmod(folder, 0o700)
    path = Path(folder) / "agent.sock"
    size = len(str(path).encode())
    if size > MAX_SOCKET_PATH_BYTES:
        shutil.rmtree(folder, ignore_errors=True)
        raise ValueError(f"{path} is {size} bytes; a socket path may be {MAX_SOCKET_PATH_BYTES}")
    return path


def remove_scratch(socket_path):
    """Deletes a `scratch_socket` folder, only when it is under the temp dir."""
    if not socket_path:
        return
    folder = os.path.realpath(os.path.dirname(str(socket_path)))
    temp = os.path.realpath(tempfile.gettempdir())
    if folder.startswith(temp + os.sep):
        shutil.rmtree(folder, ignore_errors=True)


def wait_ready(client, process, seconds, log_name):
    """Polls `tools/list` until the instance answers. Raises AssertionError
    naming `log_name` when `process` exits first, and the last OSError at the
    deadline."""
    deadline = time.monotonic() + seconds
    while True:
        assert process.poll() is None, f"Test app exited; inspect {log_name}"
        try:
            client.rpc("tools/list", {})
            return
        except OSError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.05)


def socket_pid(socket_path):
    """The pid holding the listening socket at `socket_path` (`lsof -U`, read
    only), or None. For diagnostics only."""
    names = {str(socket_path), os.path.realpath(str(socket_path))}
    try:
        listed = subprocess.run(["lsof", "-nP", "-U", "-Fpn"], capture_output=True, text=True,
                                timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    pid = None
    for line in listed.stdout.splitlines():
        if line.startswith("p"):
            pid = int(line[1:])
        elif line.startswith("n") and line[1:] in names and pid is not None:
            return pid
    return None
