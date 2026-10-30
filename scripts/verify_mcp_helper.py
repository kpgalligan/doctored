#!/usr/bin/env python3
"""MCP drive for the bundled `doctored-mcp` stdio helper and the agent socket.

Against the Debug app (`scripts/app_under_test.py`; `DR_VERIFY_APP` selects
another), every case on a scratch `DR_AGENT_SOCKET` (`mcp_client.scratch_socket`),
never the person's socket:

- BUNDLE: the bundle holds an executable `Contents/Helpers/doctored-mcp` and a
  `Contents/Resources/agent-catalog.json` with a string version and a tools array.
- SNAPSHOT: the helper, started on socket S with no app answering there, answers
  `initialize` itself (`serverInfo.name` "doctored", the snapshot's version),
  `ping`, and `tools/list` equal to the snapshot as JSON values; two seconds later
  nothing listens at S, so a list launched nothing.
- LIVE: an instance B this drive starts itself (a Popen child, the CLAUDE.md
  launch line with `DR_AGENT_SOCKET=S`) answers at S; the same helper session's
  `tools/call list_documents` is forwarded to it and is followed by no
  `list_changed` (B's list equals the snapshot), its `tools/list` equals B's live
  list as values, and `generate_mcp_reference.py --check --socket S` passes.
- CLAIM: a second instance C on S logs `not bound: another instance answers` and
  B still answers after C ends; an instance on a scratch folder made `0755` logs
  the mode refusal and leaves no socket.

The helper sends a `tools/call` only while this drive's own instance answers at
S, so it never reaches its launch path and never opens the real app (that path is
proved by `mcp-proxy/tests/stdio.rs`'s recorded launch). Every instance is a
Popen child of this drive, ended by `terminate()` then `wait(timeout=10)` in a
`finally`; the drive signals nothing else and calls no `open`.

Usage: python3 scripts/verify_mcp_helper.py [--keep]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
HELPER = "Contents/Helpers/doctored-mcp"
SNAPSHOT = "Contents/Resources/agent-catalog.json"


def isolation(folder, agent_socket):
    """The environment every child of this drive runs under."""
    return dict(os.environ, DR_BACKGROUND="1", ANTHROPIC_API_KEY="dummy",
                DR_AUTOMATION_ALERTS="record", DR_WORKING_LIMIT_BYTES="2147483648",
                DR_AGENT_SOCKET=str(agent_socket), DR_ACTIONS_DIR=str(folder / "actions"),
                DR_RECOVERY_DIR=str(folder / "recovery"), **preset_environment(folder))


class Helper:
    """The bundle's `doctored-mcp`, a Popen child; its stdout lines on a queue."""

    def __init__(self, executable, folder, agent_socket):
        self.log = (folder / "helper.log").open("w")
        self.process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=self.log,
                                        env=isolation(folder, agent_socket))
        self.lines = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.serial = 0

    def _read(self):
        for line in self.process.stdout:
            self.lines.put(line)

    def send(self, method, params=None):
        self.serial += 1
        message = {"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params or {}}
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        self.process.stdin.flush()
        return self.serial

    def next(self, seconds):
        try:
            line = self.lines.get(timeout=seconds)
        except queue.Empty:
            raise AssertionError(f"the helper said nothing within {seconds} s") from None
        message = json.loads(line)
        assert message.get("jsonrpc") == "2.0", message
        return message

    def request(self, method, params=None, seconds=30):
        serial = self.send(method, params)
        message = self.next(seconds)
        assert message.get("id") == serial, (method, message)
        assert "error" not in message, (method, message)
        return message["result"]

    def quiet(self, seconds):
        try:
            line = self.lines.get(timeout=seconds)
        except queue.Empty:
            return
        raise AssertionError(f"the helper said {line!r} where nothing was due")

    def close(self):
        try:
            self.process.stdin.close()
        except OSError:
            pass
        end(self.process)
        self.log.close()


def end(process):
    """Ends one Popen child of this drive."""
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def start_app(executable, folder, agent_socket):
    """An owned instance on `agent_socket`: (process, log path)."""
    folder.mkdir(parents=True, exist_ok=True)
    log = folder / "app.log"
    with log.open("w") as stream:
        process = subprocess.Popen(
            [str(executable), "-ApplePersistenceIgnoreState", "YES"],
            env=isolation(folder, agent_socket), stdout=stream, stderr=subprocess.STDOUT)
    return process, log


def wait_logged(process, log, needle, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        text = log.read_text(errors="replace") if log.exists() else ""
        if needle in text:
            return text
        assert process.poll() is None, f"the instance exited before logging {needle!r}; see {log}"
        time.sleep(0.1)
    raise AssertionError(f"{log} never logged {needle!r}")


def answers(agent_socket):
    try:
        Client(agent_socket, timeout=5).rpc("ping", {})
        return True
    except OSError:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder")
    arguments = parser.parse_args()
    executable = app_under_test.default_app_executable()
    app_under_test.require_fresh_app(executable)
    bundle = executable.resolve().parents[2]
    output = Path(tempfile.mkdtemp(prefix="rz-mcp-helper-"))
    print(f"scratch: {output}", flush=True)
    failures = []
    sockets = []

    def run(name, body):
        try:
            detail = body()
            print(f"PASS {name}: {detail}", flush=True)
        except Exception as error:  # noqa: BLE001 - every case reports, then the run fails
            failures.append(name)
            print(f"FAIL {name}: {type(error).__name__}: {error}", flush=True)

    helper_path = bundle / HELPER
    snapshot_path = bundle / SNAPSHOT
    snapshot = {}

    def bundle_case():
        assert os.access(helper_path, os.X_OK), f"{helper_path} is not executable"
        snapshot.update(json.loads(snapshot_path.read_text()))
        assert isinstance(snapshot.get("version"), str), snapshot.get("version")
        assert isinstance(snapshot.get("tools"), list) and snapshot["tools"], "no tools"
        return f"{helper_path.name} executable; {len(snapshot['tools'])} tools, version {snapshot['version']}"

    run("bundle", bundle_case)
    if failures:
        raise SystemExit(1)

    agent_socket = scratch_socket("rz-mcp-")
    sockets.append(agent_socket)
    helper = Helper(helper_path, output, agent_socket)
    try:
        def snapshot_case():
            started = helper.request("initialize", {"protocolVersion": "2025-06-18",
                                                    "capabilities": {},
                                                    "clientInfo": {"name": "verify", "version": "1"}})
            assert started["serverInfo"] == {"name": "doctored", "version": snapshot["version"]}, started
            assert helper.request("ping") == {}
            listed = helper.request("tools/list")
            assert listed["tools"] == snapshot["tools"], "the helper's list is not the snapshot"
            time.sleep(2)
            assert not agent_socket.exists() and not answers(agent_socket), \
                f"something listens at {agent_socket} after a list"
            return f"initialize, ping and tools/list ({len(listed['tools'])}) with nothing at the socket"

        run("snapshot", snapshot_case)

        def live_case():
            process, log = start_app(executable, output / "app-b", agent_socket)
            try:
                client = Client(agent_socket)
                wait_ready(client, process, 60, log)
                live = client.rpc("tools/list", {})["tools"]
                assert answers(agent_socket), "B answers before the helper's call"
                called = helper.request("tools/call", {"name": "list_documents", "arguments": {}})
                assert not called.get("isError"), called
                text = next(c["text"] for c in called["content"] if c["type"] == "text")
                assert "documents" in json.loads(text), text
                helper.quiet(2)
                listed = helper.request("tools/list")
                assert listed["tools"] == live, "the helper's list is not B's live list"
                assert live == snapshot["tools"], "B's live list differs from the bundle's snapshot"
                check = subprocess.run([sys.executable, str(ROOT / "scripts/generate_mcp_reference.py"),
                                        "--socket", str(agent_socket), "--check"],
                                       cwd=ROOT, capture_output=True, text=True)
                assert check.returncode == 0, check.stdout + check.stderr

                second, second_log = start_app(executable, output / "app-c", agent_socket)
                try:
                    wait_logged(second, second_log,
                                f"Doctored agent socket not bound: another instance answers at {agent_socket}")
                finally:
                    end(second)
                assert answers(agent_socket), "B stopped answering after C ended"
                assert "Doctored agent socket listening at" not in second_log.read_text(errors="replace")
            finally:
                end(process)
            return (f"list_documents forwarded with no list_changed; tools/list = B's live list "
                    f"({len(live)}); the reference page checks against B; C bound nothing and B "
                    "still answered")

        run("live and claim", live_case)
    finally:
        helper.close()

    def mode_case():
        folder = Path(tempfile.mkdtemp(prefix="rz-mcp-"))
        os.chmod(folder, 0o755)
        shared = folder / "agent.sock"
        sockets.append(shared)
        process, log = start_app(executable, output / "app-d", shared)
        try:
            wait_logged(process, log, f"Doctored agent socket not bound: {folder} is mode 0755")
        finally:
            end(process)
        assert not shared.exists(), f"{shared} was made"
        return f"{folder} is mode 0755 was logged and no socket was made"

    run("shared folder", mode_case)

    for path in sockets:
        remove_scratch(path)
    if arguments.keep:
        print(f"kept: {output}", flush=True)
    else:
        shutil.rmtree(output, ignore_errors=True)
    if failures:
        print(f"FAILED: {', '.join(failures)}", flush=True)
        raise SystemExit(1)
    print("PASS: verify_mcp_helper", flush=True)


if __name__ == "__main__":
    main()
