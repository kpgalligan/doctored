#!/usr/bin/env python3
"""Stdio conformance drive for the assistant helper, `Contents/Helpers/doctored-agent`.

The drive runs a BUILT helper as a child process of its own and plays the app's side of the
helper protocol the way `app/Sources/AssistantHelperSession.swift` and
`AssistantPanelViewController.swift` do:

- The helper gets the app's scrubbed environment: HOME, TMPDIR and PATH=/usr/bin:/bin only.
  HOME and TMPDIR are scratch folders here, so no profile or cache of the person's is read.
- `configure` is written straight after launch, with a dummy key and `api_base` pointing at
  this drive's own loopback Messages API.
- `send` is written only while the session is not busy, and `cancel` only while it is busy.
- Each `tool_request` is answered with a `tool_result` line that has the CallToolResult JSON
  spliced in raw. Once Stop was pressed, the answer is "Cancelled by the user.".

No app is launched and nothing leaves the machine. The stub API is an `http.server` on
127.0.0.1. It streams the SSE each case scripts, answers GET /v1/models/{model}, and records
every request with its exact body bytes.

Cases, one PASS/FAIL line each:
- text-turn: the events ready, turn_started, progress_delta, text_delta, context_usage and
  turn_finished. context_usage `used` is input + cache read + cache creation + output, and
  `limit` comes from the Models API. The request's headers and every body field are checked
  against what the helper must send. The helper exits 0 within the app's 2 s of stdin closing.
- settings: a `settings` message applies from the next turn (context_management and its beta;
  a null auto_compact_tokens).
- tool-round-trip: one reply carries tool search blocks, a progress thinking block and two
  tool_use blocks. Both tool_call + tool_request pairs must come before either is answered.
  The next request carries the reply verbatim: block key order and number text kept, and
  thinking signatures and tool-search blocks included. It also carries the tool results in
  tool_use order (image media type defaulted, unknown items dropped, isError kept, 200-character
  summaries). Its tools, system and first user message are byte-identical to the first request's,
  and a third request extends the second without changing its bytes.
- cancel-stream: a cancel while a reply streams ends the turn without an error and closes the
  connection. No request follows, and the next send's request holds no part of the cancelled
  reply: its text merges into the user message.
- cancel-tools: a cancel while a tool call waits. Every tool_use still gets a tool_result
  ("Cancelled by the user."), no request follows, and the next send merges into that message.
- refusal, max-tokens and context-window: the exact user-visible error strings.
- rate-limit-retry: a 429 with retry-after 0, then success. The stub sees one retry with the same
  body and the helper reports no error.
- retries-exhausted: four 429s give "API error 429: ..." after 1 + 3 attempts (maxRetries 3).
- stream-error: a mid-stream overloaded_error event reads "API error 529: Overloaded". It is not
  retried, and the next send's request holds no part of the partial reply.
- two-processes: a second process given the same configure and conversation sends a
  byte-identical first request. Two notes also report whether the second request matched too,
  and whether a configure with reordered keys changes the bytes.
- framing: a blank line is skipped, and a bad line or a send without its text answers "Bad message
  from the app: ...". A `reset` (no longer a message) is ignored. A send before configure answers
  "The assistant helper was not configured." with no turn events, and a send during a turn
  answers "The assistant is busy.".

Every wait is bounded, so a hung or crashed helper fails its case and the drive goes on. The
drive signals only the Popen children it started. A note (not a failure) reports events the app
ignores and stderr lines without the `doctored-agent: ` prefix.

Usage:
  python3 scripts/verify_agent_helper.py [--helper PATH] [--catalog PATH] [--cases a,b] [--keep]
  python3 scripts/verify_agent_helper.py --self-test   (the stub, the oracle and the child
                                                         wrapper, with no helper)
`--helper` defaults to the Debug bundle's helper and `DR_VERIFY_APP` names another bundle;
`make agent`'s copy is driven with `--helper build/agent/doctored-agent`. The tool
catalog is the bundle's Contents/Resources/agent-catalog.json minus close_document, as the panel
configures it. Without that file a small built-in catalog is used. A helper older than agent/
Package.swift, Package.resolved or Sources is refused; `DR_ALLOW_STALE_APP=1` drives it anyway.
Exits 0 only when every case passed. `make gate`'s `launch` step runs text-turn and framing against
the Release bundle's helper (`scripts/gate.py`, `helper_failure`).
"""
from __future__ import annotations

import argparse
import base64
import collections
import http.client
import http.server
import json
import os
from pathlib import Path
import queue
import random
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
HELPER_IN_BUNDLE = "Contents/Helpers/doctored-agent"
CATALOG_IN_BUNDLE = "Contents/Resources/agent-catalog.json"

# The app's side, as AssistantHelperSession.open builds `configure`. Texts are representative;
# shapes and fixed values are the app's.
DUMMY_KEY = "sk-ant-dummy-verify-agent-helper"
MODEL = "claude-opus-5-5"  # AssistantPanelViewController.ensureSession's default
CORE_TOOLS = ["render", "get_document", "list_documents", "open_document"]  # AssistantHelperPrompt
PANEL_EXCLUDED_TOOLS = {"close_document"}  # AssistantHelperSession.panelExcludedTools
SYSTEM = ("You are the assistant built into Doctored, a macOS layered image editor. The user is "
          "looking at document id 1: \"Harbour — dusk\" (1600×1200 px, 3 layer(s)). Operate on this "
          "document unless told otherwise — pass document_id 1 on every tool call.")
SEARCH_GUIDE = ("Most of your tools are loaded on demand. Search for them with tool_search_tool_bm25, "
                "in plain words describing the operation (\"gaussian blur\", \"select the sky\").")
COMPACTION_INSTRUCTIONS = ("Summarize this conversation so the assistant can continue it from the "
                           "summary alone. Do not call tools; respond with the summary text only.")
SETTINGS = {"auto_compact_tokens": 300_000, "clear_tool_results": False}  # helperJSON's defaults
CANCELLED_RESULT = '{"content":[{"type":"text","text":"Cancelled by the user."}],"isError":true}'

# What the helper must send and say (designs/assistant-helper.md, "Protocol"; API version 2026-10-21).
API_VERSION = "2026-10-21"
BASE_BETAS = {"thinking-display-updates-2026-08-18", "server-side-fallback-2026-07-01",
              "compact-2026-09-04"}
CLEARING_BETA = "context-management-2025-06-27"
TOOL_SEARCH = {"type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25"}
CLEAR_TOOL_USES = {"edits": [{"type": "clear_tool_uses_20250919",
                              "trigger": {"type": "input_tokens", "value": 150_000},
                              "keep": {"type": "tool_uses", "value": 5},
                              "clear_at_least": {"type": "input_tokens", "value": 50_000}}]}
FIXED_FIELDS = {"model": MODEL, "max_tokens": 32_000, "stream": True,
                "cache_control": {"type": "ephemeral"}, "output_config": {"effort": "medium"},
                "thinking": {"type": "adaptive", "display": "updates"}, "fallbacks": "default"}
BODY_FIELDS = set(FIXED_FIELDS) | {"system", "tools", "messages", "context_management"}
SAMPLING_FIELDS = {"temperature", "top_p", "top_k"}  # a 400 on 2026-10-21
REFUSAL_ERROR = "The model declined this request."
MAX_TOKENS_ERROR = "The reply was cut off at the output limit."
CONTEXT_WINDOW_ERROR = "The reply was cut off because the conversation filled the model's context window."
NOT_CONFIGURED_ERROR = "The assistant helper was not configured."
BUSY_ERROR = "The assistant is busy."
BAD_MESSAGE_PREFIX = "Bad message from the app: "
STDERR_PREFIX = "doctored-agent: "
# The events AssistantPanelViewController.handleEvent and the session act on, plus `ready`.
APP_EVENTS = {"ready", "turn_started", "turn_finished", "text_delta", "progress_delta",
              "context_usage", "tool_call", "tool_request", "tool_result", "error",
              "compaction_started", "compacted", "compaction_failed", "tool_results_cleared"}
REMOVED_EVENTS = {"prices", "cost", "message_done"}  # dropped with pricing (designs/assistant-helper.md, AH-4)
MODEL_LIMIT = 1_000_000

# Bounds. A cold launch, Slaude's client and the first request fit well inside TURN_SECONDS.
TURN_SECONDS = 45.0
EVENT_SECONDS = 10.0
HOLD_SECONDS = 10.0  # for the second tool_request of a response, while the first is held
CANCEL_SECONDS = 15.0
EXIT_SECONDS = 2.0  # AssistantHelperSession.close's grace before SIGTERM
QUIET_SECONDS = 1.0
STALL_SECONDS = 60.0


# ---- Exact JSON: key order and number text kept, as Slaude's RawJSON keeps them.

class Num:
    """A JSON number by its text, so 2.50 and 2.5 differ."""
    __slots__ = ("text",)

    def __init__(self, text):
        self.text = text

    def __eq__(self, other):
        return isinstance(other, Num) and other.text == self.text

    def __hash__(self):
        return hash(self.text)

    def __int__(self):
        return int(self.text)

    def __repr__(self):
        return self.text


def load_exact(text):
    return json.loads(text, parse_int=Num, parse_float=Num)


def dump_exact(value):
    """Compact JSON of a `load_exact` value: its key order and number text, strings re-escaped."""
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key, ensure_ascii=False) + ":" + dump_exact(item)
                              for key, item in value.items()) + "}"
    if isinstance(value, list):
        return "[" + ",".join(dump_exact(item) for item in value) + "]"
    if isinstance(value, Num):
        return value.text
    return json.dumps(value, ensure_ascii=False)


_DECODER = json.JSONDecoder()
_SPACE = " \t\n\r"


def _skip(text, index):
    while index < len(text) and text[index] in _SPACE:
        index += 1
    return index


def raw_members(text, index=0):
    """The members of the JSON object or array at `text[index:]` as their exact source text:
    {key: text} in document order for an object, [text] for an array."""
    index = _skip(text, index)
    opener = text[index]
    closer = {"{": "}", "[": "]"}[opener]
    members = {} if opener == "{" else []
    index = _skip(text, index + 1)
    if text[index] == closer:
        return members
    while True:
        if opener == "{":
            key, index = _DECODER.raw_decode(text, index)
            index = _skip(text, index)
            if text[index] != ":":
                raise ValueError(f"no ':' after key {key!r} at {index}")
            index = _skip(text, index + 1)
        _, end = _DECODER.raw_decode(text, index)
        if opener == "{":
            members[key] = text[index:end]
        else:
            members.append(text[index:end])
        index = _skip(text, end)
        if text[index] == ",":
            index = _skip(text, index + 1)
            continue
        if text[index] == closer:
            return members
        raise ValueError(f"malformed JSON at offset {index}")


def first_difference(got, expected):
    """Where two texts part, with context, for a failure line."""
    at = next((i for i, (a, b) in enumerate(zip(got, expected)) if a != b), min(len(got), len(expected)))
    lo = max(0, at - 60)
    return (f"they differ at offset {at} (lengths {len(got)} and {len(expected)}): "
            f"got …{got[lo:at + 60]!r}… where …{expected[lo:at + 60]!r}… was expected")


# ---- The oracle: what a reply's events build, by the rule the helper's stream must follow
# (Slaude's JSONMessageAccumulator), restated here independently.

def accumulate(events):
    """(message, complete) for SSE `events` [(name, data text)]: message_start's message, each
    block as content_block_start gave it with its text, thinking and signature deltas applied,
    a block's input_json_delta parts parsed at its content_block_stop, and message_delta's stop
    fields and usage totals."""
    head, blocks, inputs, complete = None, [], {}, False
    for name, text in events:
        if name in ("ping", "error"):
            continue
        event = load_exact(text)
        kind = event.get("type")
        if head is None:
            if kind == "message_start":
                head = event["message"]
                blocks = list(head.get("content") or [])
            continue
        if kind == "content_block_start":
            blocks.append(event["content_block"])
        elif kind == "content_block_delta":
            index = int(event["index"])
            block, delta = blocks[index], event["delta"]
            change = delta.get("type")
            if change == "text_delta" and block.get("type") == "text":
                block["text"] = block.get("text", "") + delta["text"]
            elif change == "thinking_delta" and block.get("type") == "thinking":
                block["thinking"] = block.get("thinking", "") + delta["thinking"]
            elif change == "signature_delta" and block.get("type") == "thinking":
                block["signature"] = delta["signature"]
            elif change == "input_json_delta":
                inputs[index] = inputs.get(index, "") + delta["partial_json"]
        elif kind == "content_block_stop":
            index = int(event["index"])
            if inputs.get(index):
                blocks[index]["input"] = load_exact(inputs.pop(index))
        elif kind == "message_delta":
            for key, value in (event.get("delta") or {}).items():
                if key in ("stop_reason", "stop_sequence", "stop_details") or value is not None:
                    head[key] = value
            totals = head.setdefault("usage", {})
            for key, value in (event.get("usage") or {}).items():
                if key == "output_tokens" or value is not None:
                    totals[key] = value
        elif kind == "message_stop":
            complete = True
    if head is not None:
        head["content"] = blocks
    return head, complete


def context_tokens(usage):
    """The context meter's figure: input + cache read + cache creation + output."""
    return sum(int(usage.get(key) or 0) for key in
               ("input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"))


# ---- Scripted replies.

def usage(input_tokens, cache_read=0, cache_creation=0):
    return {"input_tokens": input_tokens, "cache_creation_input_tokens": cache_creation,
            "cache_read_input_tokens": cache_read,
            "cache_creation": {"ephemeral_5m_input_tokens": cache_creation, "ephemeral_1h_input_tokens": 0},
            "output_tokens": 1, "service_tier": "standard"}


def error_body(error_type, message):
    return {"type": "error", "error": {"type": error_type, "message": message}, "request_id": "req_stub"}


def _data(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


class Stream:
    """An SSE reply: its events in order; `stall` holds the body open after them, sending SSE
    comments, until the client goes away (`Recorded.disconnected`) or the stub closes."""

    def __init__(self, events, stall=False):
        self.events = list(events)
        self.stall = stall

    def serve(self, handler, record, stub):
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream; charset=utf-8")
        handler.send_header("Cache-Control", "no-cache")
        handler.send_header("Transfer-Encoding", "chunked")
        handler.send_header("request-id", f"req_stub_{record.index}")
        handler.end_headers()
        try:
            for name, data in self.events:
                handler.chunk(f"event: {name}\ndata: {data}\n\n".encode("utf-8"))
            if self.stall:
                deadline = time.monotonic() + STALL_SECONDS
                while time.monotonic() < deadline:
                    handler.chunk(b": the stub holds this reply open\n\n")
                    if stub.released.wait(0.2):
                        break
                handler.close_connection = True  # never finished: the body breaks off
                return
            handler.chunk(b"")
        except OSError:
            record.disconnected.set()
            handler.close_connection = True


class Status:
    """An HTTP error reply with the API's error body."""

    def __init__(self, status, error_type, message, headers=None):
        self.status, self.error_type, self.message = status, error_type, message
        self.headers = headers or {}

    def serve(self, handler, record, stub):
        handler.send_json(self.status, error_body(self.error_type, self.message), record, self.headers)


class Reply:
    """Builds one streamed Messages reply as the API sends it: message_start, a ping, the
    blocks with their deltas, then message_delta + message_stop, an error event, or a stall."""

    def __init__(self, message_id, reply_usage):
        self.events = []
        self.index = 0
        self._event("message_start", {"type": "message_start", "message": {
            "id": message_id, "type": "message", "role": "assistant", "model": MODEL, "content": [],
            "stop_reason": None, "stop_sequence": None, "usage": reply_usage}})
        self.events.append(("ping", '{"type":"ping"}'))

    def _event(self, name, data):
        self.events.append((name, _data(data)))

    def _start(self, block):
        self._event("content_block_start", {"type": "content_block_start", "index": self.index,
                                            "content_block": block})

    def _delta(self, delta):
        self._event("content_block_delta", {"type": "content_block_delta", "index": self.index,
                                            "delta": delta})

    def _stop(self):
        self._event("content_block_stop", {"type": "content_block_stop", "index": self.index})
        self.index += 1

    def thinking(self, chunks, signature):
        """A thinking block: a progress update when `chunks` has text, reasoning when empty."""
        self._start({"type": "thinking", "thinking": "", "signature": ""})
        for chunk in chunks:
            self._delta({"type": "thinking_delta", "thinking": chunk})
        self._delta({"type": "signature_delta", "signature": signature})
        self._stop()
        return self

    def text(self, chunks, close=True):
        self._start({"type": "text", "text": ""})
        for chunk in chunks:
            self._delta({"type": "text_delta", "text": chunk})
        if close:
            self._stop()
        return self

    def tool_use(self, tool_id, name, input_chunks, kind="tool_use", extra=None):
        block = {"type": kind, "id": tool_id, "name": name, "input": {}}
        block.update(extra or {})
        self._start(block)
        for chunk in input_chunks:
            self._delta({"type": "input_json_delta", "partial_json": chunk})
        self._stop()
        return self

    def whole(self, block):
        self._start(block)
        self._stop()
        return self

    def end(self, stop_reason, output_tokens):
        self._event("message_delta", {"type": "message_delta",
                                      "delta": {"stop_reason": stop_reason, "stop_sequence": None},
                                      "usage": {"output_tokens": output_tokens}})
        self._event("message_stop", {"type": "message_stop"})
        return Stream(self.events)

    def error(self, error_type, message):
        self.events.append(("error", _data({"type": "error",
                                            "error": {"type": error_type, "message": message}})))
        return Stream(self.events)

    def stall(self):
        return Stream(self.events, stall=True)


# ---- The stub Messages API.

class Recorded:
    """One request the stub received."""

    def __init__(self, index, method, path, headers, body):
        self.index, self.method, self.path, self.body = index, method, path, body
        self.route = urllib.parse.urlsplit(path).path
        self.headers = [(name.lower(), value) for name, value in headers.items()]
        self.received = time.monotonic()
        self.disconnected = threading.Event()
        self._members = None

    @property
    def text(self):
        return self.body.decode("utf-8")

    def header(self, name):
        values = [value for key, value in self.headers if key == name]
        return ",".join(values) if values else None

    def json(self):
        return json.loads(self.text)

    def members(self):
        if self._members is None:
            self._members = raw_members(self.text)
        return self._members

    def messages(self):
        return raw_members(self.members()["messages"])


class StubHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    stub = None  # set on the per-stub subclass

    def log_message(self, *arguments):
        pass

    def chunk(self, data):
        self.wfile.write(f"{len(data):X}\r\n".encode() + data + b"\r\n")
        self.wfile.flush()

    def send_json(self, status, value, record, headers=None):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("request-id", f"req_stub_{record.index}")
        for name, text in (headers or {}).items():
            self.send_header(name, text)
        self.end_headers()
        self.wfile.write(data)

    def _body(self):
        if "chunked" in (self.headers.get("Transfer-Encoding") or "").lower():
            parts = []
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    return b"".join(parts)
                parts.append(self.rfile.read(size))
                self.rfile.readline()
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def do_GET(self):
        record = self.stub.record("GET", self.path, self.headers, b"")
        if record.route.startswith("/v1/models/"):
            model = urllib.parse.unquote(record.route[len("/v1/models/"):])
            self.send_json(200, {"type": "model", "id": model, "display_name": "Stub model",
                                 "created_at": "2026-01-01T00:00:00Z", "lifecycle": "active",
                                 "deprecated_at": None, "retires_at": None,
                                 "max_input_tokens": self.stub.model_limit, "max_tokens": 128_000}, record)
        else:
            self.send_json(404, error_body("not_found_error", f"the stub serves no {record.route}"), record)

    def do_POST(self):
        record = self.stub.record("POST", self.path, self.headers, self._body())
        if record.route != "/v1/messages":
            self.send_json(404, error_body("not_found_error", f"the stub serves no {record.route}"), record)
            return
        reply = self.stub.next_reply()
        if reply is None:
            self.stub.problems.append(f"request {record.index} had no scripted reply")
            self.send_json(400, error_body("invalid_request_error",
                                           "the stub has no scripted reply for this request"), record)
            return
        reply.serve(self, record, self.stub)


class StubServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False
    stub = None

    def handle_error(self, request, client_address):
        """A client that drops a kept-alive connection (a helper ended) is no fault; any other
        exception in a handler is a problem of the case's."""
        error = sys.exc_info()[1]
        if not isinstance(error, (ConnectionError, TimeoutError)):
            self.stub.problems.append(f"the stub failed serving {client_address}: {type(error).__name__}: {error}")


class StubAPI:
    """A loopback Messages API: scripted replies in order, every request recorded."""

    def __init__(self, model_limit=MODEL_LIMIT):
        self.model_limit = model_limit
        self.replies = collections.deque()
        self.requests = []
        self.problems = []
        self.condition = threading.Condition()
        self.released = threading.Event()
        handler = type("Handler", (StubHandler,), {"stub": self})
        self.server = StubServer(("127.0.0.1", 0), handler)
        self.server.stub = self
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1},
                                       daemon=True)
        self.thread.start()

    @property
    def base_url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def script(self, *replies):
        with self.condition:
            self.replies.extend(replies)

    def record(self, method, path, headers, body):
        with self.condition:
            record = Recorded(len(self.requests), method, path, headers, body)
            self.requests.append(record)
            self.condition.notify_all()
            return record

    def next_reply(self):
        with self.condition:
            return self.replies.popleft() if self.replies else None

    def posts(self):
        with self.condition:
            return [r for r in self.requests if r.method == "POST" and r.route == "/v1/messages"]

    def wait_posts(self, count, seconds):
        with self.condition:
            self.condition.wait_for(lambda: len(self.posts()) >= count, timeout=seconds)
        return self.posts()

    def wait_for(self, predicate, seconds):
        with self.condition:
            self.condition.wait_for(lambda: any(predicate(r) for r in self.requests), timeout=seconds)
            return next((r for r in self.requests if predicate(r)), None)

    def close(self):
        self.released.set()
        self.server.shutdown()
        self.server.server_close()


# ---- The helper under test, and the app's side of its session.

class Silent(AssertionError):
    pass


class Helper:
    """The helper, a Popen child of this drive with the app's environment. Its stdout lines are
    read on one thread and its stdin is written on another, as the app's writeQueue does, so a
    multi-megabyte line never blocks the drive. `stderr_path` defaults to `<folder>/<label>.stderr`;
    scripts/live_assistant_acceptance.py names its own."""

    def __init__(self, executable, folder, label, stderr_path=None):
        self.label = label
        folder.mkdir(parents=True, exist_ok=True)
        home, temporary = folder / f"{label}-home", folder / f"{label}-tmp"
        home.mkdir(exist_ok=True)
        temporary.mkdir(exist_ok=True)
        self.stderr_path = Path(stderr_path) if stderr_path else folder / f"{label}.stderr"
        self._stderr = self.stderr_path.open("wb")
        environment = {"HOME": str(home), "TMPDIR": f"{temporary}/", "PATH": "/usr/bin:/bin"}
        self.process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self._stderr, env=environment, cwd=str(folder))
        self.lines = queue.Queue()
        self.writes = queue.Queue()
        self.events = []
        self.violations = []
        self.ended = False
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._write, daemon=True).start()

    def _read(self):
        for line in self.process.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def _write(self):
        while True:
            data = self.writes.get()
            if data is None:
                try:
                    self.process.stdin.close()
                except OSError:
                    pass
                return
            try:
                self.process.stdin.write(data)
                self.process.stdin.flush()
            except OSError:
                pass  # the helper is gone; its exit is reported by the reader

    def write_line(self, line):
        self.writes.put((line + "\n").encode("utf-8"))

    def write(self, message):
        self.write_line(json.dumps(message, ensure_ascii=False, separators=(",", ":")))

    def next(self, seconds):
        """The next protocol event; a stdout line that is not one is recorded as a violation."""
        deadline = time.monotonic() + seconds
        while True:
            try:
                line = self.lines.get(timeout=max(deadline - time.monotonic(), 0.001))
            except queue.Empty:
                raise Silent(f"the helper said nothing within {seconds:g} s") from None
            if line is None:
                self.lines.put(None)
                try:
                    status = self.process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    status = "unknown"
                raise AssertionError(f"the helper closed stdout (exit status {status})")
            try:
                event = json.loads(line)
            except ValueError:
                event = None
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                self.violations.append(line)
                continue
            self.events.append(event)
            return event

    def drain(self, seconds):
        """Every event within `seconds`, without failing on silence or exit."""
        events, deadline = [], time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                events.append(self.next(deadline - time.monotonic()))
            except AssertionError:
                break
        return events

    def close_stdin_and_wait(self, seconds):
        """Closes stdin as AssistantHelperSession.close does: the exit status, or None."""
        self.writes.put(None)
        try:
            return self.process.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            return None

    def stderr_tail(self, count=6):
        try:
            lines = [line for line in self.stderr_path.read_text(errors="replace").splitlines() if line.strip()]
        except OSError:
            return ""
        return " | ".join(lines[-count:])

    def stderr_lines(self):
        try:
            return [line for line in self.stderr_path.read_text(errors="replace").splitlines() if line.strip()]
        except OSError:
            return []

    def end(self):
        """Closes stdin, gives the app's 2 s, then SIGTERM, then SIGKILL: this child only."""
        if self.ended:
            return
        self.ended = True
        self.writes.put(None)
        try:
            self.process.wait(timeout=EXIT_SECONDS)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self._stderr.close()


class App:
    """One Assistant panel's session as AssistantHelperSession runs it: the busy and
    cancel-requested flags, the send/cancel guards and the tool answers."""

    def __init__(self, helper):
        self.helper = helper
        self.busy = False
        self.cancel_requested = False

    def send(self, text):
        assert not self.busy, "the drive sent while busy, which the app never does"
        self.busy = True
        self.helper.write({"type": "send", "text": text})

    def cancel(self):
        if self.busy:
            self.cancel_requested = True
            self.helper.write({"type": "cancel"})

    def update_settings(self, settings):
        self.helper.write({"type": "settings", "settings": settings})

    def answer(self, request, result):
        """runTool: the result JSON spliced in raw; "Cancelled by the user." after Stop."""
        call_id = request.get("call_id")
        assert isinstance(call_id, str), f"tool_request call_id {call_id!r} is not a string (the app drops it)"
        assert isinstance(request.get("name"), str), f"tool_request name {request.get('name')!r} is not a string"
        if self.cancel_requested:
            result = CANCELLED_RESULT
        self.helper.write_line('{"type":"tool_result","call_id":' + json.dumps(call_id) +
                               ',"result":' + result + "}")

    def next(self, seconds):
        event = self.helper.next(seconds)
        if event["type"] == "turn_started":
            self.busy = True
            self.cancel_requested = False
        elif event["type"] == "turn_finished":
            self.busy = False
        return event

    def until(self, kind, seconds, events):
        """Reads into `events` until an event of `kind`, which it returns."""
        deadline = time.monotonic() + seconds
        while True:
            try:
                event = self.next(deadline - time.monotonic())
            except Silent:
                raise AssertionError(f"no {kind} within {seconds:g} s; events: {brief(events)}") from None
            events.append(event)
            if event["type"] == kind:
                return event
            if event["type"] == "turn_finished":
                raise AssertionError(f"the turn finished before any {kind}: {brief(events)}")

    def turn(self, seconds, answer=None):
        """The events through turn_finished; each tool_request answered with `answer(request)`."""
        events, deadline = [], time.monotonic() + seconds
        while True:
            try:
                event = self.next(deadline - time.monotonic())
            except Silent:
                raise AssertionError(f"no turn_finished within {seconds:g} s; events: {brief(events)}") from None
            events.append(event)
            if event["type"] == "tool_request":
                assert answer is not None, f"an unexpected tool_request for {event.get('name')!r}"
                self.answer(event, answer(event))
            if event["type"] == "turn_finished":
                return events


def brief(events, limit=18):
    runs = []
    for event in events:
        kind = event.get("type")
        if kind == "error":
            kind = f"error({event.get('message')!r})"
        if runs and runs[-1][0] == kind:
            runs[-1][1] += 1
        else:
            runs.append([kind, 1])
    parts = [kind if count == 1 else f"{kind}×{count}" for kind, count in runs]
    return (", ".join(parts[:limit]) + (", …" if len(parts) > limit else "")) or "(none)"


def configure_line(base_url, catalog, settings=None):
    return json.dumps({"type": "configure", "api_key": DUMMY_KEY, "model": MODEL, "system": SYSTEM,
                       "tools": catalog, "settings": settings or SETTINGS, "core_tools": CORE_TOOLS,
                       "search_guide": SEARCH_GUIDE, "compaction_instructions": COMPACTION_INSTRUCTIONS,
                       "api_base": base_url}, ensure_ascii=False, separators=(",", ":"))


class Context:
    """One case's helpers and stubs, ended in its `finally`, and its notes."""

    def __init__(self, executable, catalog, folder):
        self.executable, self.catalog, self.folder = executable, catalog, folder
        self.helpers, self.stubs, self.notes = [], [], []

    def stub(self):
        stub = StubAPI()
        self.stubs.append(stub)
        return stub

    def helper(self, label="helper"):
        helper = Helper(self.executable, self.folder, label)
        self.helpers.append(helper)
        return helper

    def app(self, stub, label="helper", settings=None, line=None):
        """A helper configured as AssistantHelperSession.open configures it."""
        app = App(self.helper(label))
        app.helper.write_line(line or configure_line(stub.base_url, self.catalog, settings))
        return app

    def audit(self):
        """What every helper of the case must have kept to, and the notes about it."""
        notes = []
        for helper in self.helpers:
            assert not helper.violations, (f"{helper.label} wrote {len(helper.violations)} stdout line(s) that "
                                           f"are not protocol events, first {helper.violations[0][:160]!r}")
            kinds = {event["type"] for event in helper.events}
            removed = sorted(kinds & REMOVED_EVENTS)
            assert not removed, f"{helper.label} still emits the removed pricing events {removed}"
            ignored = sorted(kinds - APP_EVENTS - REMOVED_EVENTS)
            if ignored:
                notes.append(f"{helper.label} emits events the app ignores: {', '.join(ignored)}")
            unprefixed = [line for line in helper.stderr_lines() if not line.startswith(STDERR_PREFIX)]
            if unprefixed:
                notes.append(f"{helper.label}: {len(unprefixed)} stderr line(s) without {STDERR_PREFIX!r}, "
                             f"first {unprefixed[0][:120]!r}")
        for stub in self.stubs:
            assert not stub.problems, "; ".join(stub.problems)
        return notes

    def cleanup(self):
        for helper in self.helpers:
            try:
                helper.end()
            except Exception:  # noqa: BLE001 - cleanup goes on to the next child
                pass
        for stub in self.stubs:
            try:
                stub.close()
            except Exception:  # noqa: BLE001
                pass


# ---- Checks shared by the cases.

def user(*texts):
    return {"role": "user", "content": [{"type": "text", "text": text} for text in texts]}


def check_tools(tools, catalog):
    assert isinstance(tools, list) and tools, "the request has no tools"
    assert tools[0] == TOOL_SEARCH, f"tools[0] is {tools[0]!r}, not the BM25 tool search tool"
    assert len(tools) == len(catalog) + 1, (f"{len(tools) - 1} catalog tools sent where {len(catalog)} were "
                                            "configured")
    for entry, tool in zip(catalog, tools[1:]):
        name = entry["name"]
        assert tool.get("name") == name, \
            f"tools leave catalog order: {tool.get('name')!r} where {name!r} was configured"
        assert tool.get("description") == entry.get("description"), f"{name}'s description is not the configured one"
        assert tool.get("input_schema") == entry.get("inputSchema"), f"{name}'s input_schema is not its inputSchema"
        extra = sorted(set(tool) - {"name", "description", "input_schema", "defer_loading"})
        assert not extra, f"{name} carries {extra}"
        if name in CORE_TOOLS:
            assert "defer_loading" not in tool, f"the core tool {name} carries defer_loading {tool['defer_loading']!r}"
        else:
            assert tool.get("defer_loading") is True, f"{name} is not sent with defer_loading true"


def check_request(record, catalog, clearing=False):
    """The headers and fields every Messages request of the helper carries; the parsed body."""
    assert (record.method, record.route) == ("POST", "/v1/messages"), f"{record.method} {record.path}"
    version = record.header("anthropic-version")
    assert version == API_VERSION, f"anthropic-version {version!r}, expected {API_VERSION}"
    authorization, api_key = record.header("authorization"), record.header("x-api-key")
    assert authorization == f"Bearer {DUMMY_KEY}" or api_key == DUMMY_KEY, \
        f"the configured key is not the credential (authorization {authorization!r}, x-api-key {api_key!r})"
    betas = {beta.strip() for beta in (record.header("anthropic-beta") or "").split(",") if beta.strip()}
    expected = BASE_BETAS | ({CLEARING_BETA} if clearing else set())
    assert betas == expected, f"anthropic-beta {sorted(betas)}, expected {sorted(expected)}"
    assert DUMMY_KEY not in record.text, "the API key is in the request body"
    body = record.json()
    fields = set(body)
    assert not fields & SAMPLING_FIELDS, f"sampling fields {sorted(fields & SAMPLING_FIELDS)} (a 400 on {API_VERSION})"
    assert fields <= BODY_FIELDS, f"unexpected request fields {sorted(fields - BODY_FIELDS)}"
    missing = sorted(BODY_FIELDS - {"context_management"} - fields)
    assert not missing, f"the request lacks {missing}"
    for field, value in FIXED_FIELDS.items():
        assert body[field] == value, f"{field} is {body[field]!r}, expected {value!r}"
    assert body["system"] == [{"type": "text", "text": f"{SYSTEM}\n\n{SEARCH_GUIDE}",
                               "cache_control": {"type": "ephemeral"}}], \
        "system is not one text block of configure.system, a blank line and search_guide, " \
        "with the static prefix's cache breakpoint"
    if clearing:
        assert body.get("context_management") == CLEAR_TOOL_USES, \
            f"context_management is {body.get('context_management')!r}, expected {CLEAR_TOOL_USES!r}"
    else:
        assert "context_management" not in body, "context_management sent with clearing off"
    check_tools(body["tools"], catalog)
    assert isinstance(body["messages"], list), "messages is not an array"
    return body


def check_turn(events, errors=()):
    """One turn's protocol: one turn_started before the turn's own events, turn_finished last,
    exactly the expected error events, and the field types the panel reads."""
    kinds = [event["type"] for event in events]
    assert kinds.count("turn_started") == 1, f"{kinds.count('turn_started')} turn_started: {brief(events)}"
    assert kinds.count("turn_finished") == 1 and kinds[-1] == "turn_finished", \
        f"turn_finished not last: {brief(events)}"
    early = [kind for kind in kinds[:kinds.index("turn_started")] if kind not in ("ready", "context_usage")]
    assert not early, f"{early} before turn_started"
    got = [event.get("message") for event in events if event["type"] == "error"]
    assert got == list(errors), f"error events {got!r}, expected {list(errors)!r}"
    for event in events:
        kind = event["type"]
        if kind in ("text_delta", "progress_delta"):
            assert isinstance(event.get("text"), str), f"{kind} text {event.get('text')!r}"
        elif kind == "context_usage":
            used, limit = event.get("used"), event.get("limit")
            assert type(used) is int, f"context_usage used {used!r} is not a JSON integer"
            assert limit is None or type(limit) is int, f"context_usage limit {limit!r}"
        elif kind == "tool_call":
            assert isinstance(event.get("name"), str) and isinstance(event.get("arguments"), dict), \
                f"tool_call {event!r}"
        elif kind == "tool_result":
            assert isinstance(event.get("name"), str) and type(event.get("is_error")) is bool \
                and isinstance(event.get("summary"), str), f"tool_result {event!r}"
        elif kind == "tool_request":
            assert isinstance(event.get("arguments_json"), str), f"tool_request arguments_json {event!r}"


def texts(events, kind):
    return "".join(event["text"] for event in events if event["type"] == kind)


def check_exact(raw, expected, what):
    got, want = dump_exact(load_exact(raw)), dump_exact(expected)
    assert got == want, f"{what} is not the reply as received: {first_difference(got, want)}"


def without_false_is_error(message):
    content = [{key: value for key, value in block.items() if not (key == "is_error" and value is False)}
               for block in message.get("content", [])]
    return {**message, "content": content}


# ---- The scripted conversation of the tool round trip, shared with two-processes.

BLUR_REQUEST = "Blur the background, then show me."
FILTER_REFUSAL = ("Radius out of range: apply_filter takes a gaussian_blur radius of at most 2.0 px on this "
                  "layer and 2.5 px was asked for. Pass a smaller radius or duplicate the layer first; the "
                  "layer is unchanged and no undo step was added.")
IMAGE_DATA = base64.b64encode(random.Random(20261103).randbytes(1_500_000)).decode()  # a render-sized line
RENDER_RESULT = json.dumps({"content": [{"type": "image", "data": IMAGE_DATA},
                                        {"type": "resource", "resource": {"uri": "doctored://document/1"}}]},
                           separators=(",", ":"))
FILTER_RESULT = json.dumps({"content": [{"type": "text", "text": FILTER_REFUSAL}], "isError": True},
                           separators=(",", ":"))
SIGNATURES = {"reasoning": "EuYBCkYIBxgCKkBv3reasoning0signature+stub/AAAA==",
              "progress": "EqQBCkYIBxgCKkCprogress0signature+stub/BBBB==",
              "other": "ErUBCkYIBxgCKkDother0signature+stub/CCCC=="}


def deferred_tool(catalog):
    names = [entry["name"] for entry in catalog]
    if "apply_filter" in names:
        return "apply_filter"
    return next(name for name in names if name not in CORE_TOOLS)


def tool_round_trip_replies(deferred):
    first = (Reply("msg_tools_1", usage(2048, cache_creation=1800))
             .thinking([], SIGNATURES["reasoning"])
             .tool_use("srvtoolu_01S", "tool_search_tool_bm25", ['{"query": ', '"blur filter"}'],
                       kind="server_tool_use")
             .whole({"type": "tool_search_tool_result", "tool_use_id": "srvtoolu_01S",
                     "content": {"type": "tool_search_tool_search_result",
                                 "tool_references": [{"type": "tool_reference", "tool_name": deferred}]}})
             .thinking(["I'll blur the background, ", "then look at it."], SIGNATURES["progress"])
             .text(["Blurring now."])
             .tool_use("toolu_01A", deferred, ['{"filter": "gaussian_blur", ', '"radius": 2.50, "document_id": 1}'],
                       extra={"caller": {"type": "direct"}})
             .tool_use("toolu_01B", "render", ['{"document_id": 1}'])
             .end("tool_use", 187))
    second = (Reply("msg_tools_2", usage(310, cache_read=3850))
              .text(["Done — the background ", "is blurred."]).end("end_turn", 23))
    return first, second


def tool_answers(deferred):
    return {deferred: FILTER_RESULT, "render": RENDER_RESULT}


def expected_tool_results():
    return {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "toolu_01A",
         "content": [{"type": "text", "text": FILTER_REFUSAL}], "is_error": True},
        {"type": "tool_result", "tool_use_id": "toolu_01B",
         "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png",
                                                  "data": IMAGE_DATA}}]}]}


# ---- Cases.

def case_text_turn(ctx):
    stub = ctx.stub()
    reply = (Reply("msg_text_1", usage(1210, cache_read=4096, cache_creation=320))
             .thinking(["Reading the ", "request."], SIGNATURES["progress"])
             .text(["Hello — ", "café ✓ ", "👋 ready."]).end("end_turn", 41))
    stub.script(reply)
    app = ctx.app(stub)
    app.send("Say hello.")  # straight after configure, as the first Send does
    events = app.turn(TURN_SECONDS)
    check_turn(events)
    every = app.helper.events
    kinds = [event["type"] for event in every]
    assert kinds.count("ready") == 1, f"{kinds.count('ready')} ready events for one configure"
    assert kinds.index("ready") < kinds.index("turn_started"), "ready came after turn_started"
    progress, text = texts(events, "progress_delta"), texts(events, "text_delta")
    assert progress == "Reading the request.", f"progress_delta text {progress!r}"
    assert text == "Hello — café ✓ 👋 ready.", f"text_delta text {text!r}"
    last_progress = max(i for i, e in enumerate(events) if e["type"] == "progress_delta")
    first_text = min(i for i, e in enumerate(events) if e["type"] == "text_delta")
    assert last_progress < first_text, "progress_delta came after text_delta"
    message, _ = accumulate(reply.events)
    used = context_tokens(message["usage"])
    start = [e["type"] for e in events].index("turn_started")
    in_turn = [e["used"] for e in events[start:] if e["type"] == "context_usage"]
    assert used in in_turn, \
        f"no context_usage of {used} (input + cache read + cache creation + output) in the turn: {in_turn}"
    if not any(e["type"] == "context_usage" and e.get("limit") == MODEL_LIMIT for e in every):
        app.helper.drain(5.0)
    limits = [e.get("limit") for e in app.helper.events if e["type"] == "context_usage"]
    assert MODEL_LIMIT in limits, f"no context_usage with the Models API's limit {MODEL_LIMIT}: limits {limits}"
    models = stub.wait_for(lambda r: r.method == "GET" and r.route.startswith("/v1/models/"), 5.0)
    assert models is not None and models.route == f"/v1/models/{MODEL}", "the helper never asked the Models API"
    posts = stub.posts()
    assert len(posts) == 1, f"{len(posts)} Messages requests for a one-reply turn"
    body = check_request(posts[0], ctx.catalog)
    assert body["messages"] == [user("Say hello.")], f"messages {body['messages']!r}"
    status = app.helper.close_stdin_and_wait(EXIT_SECONDS)
    assert status == 0, f"the helper did not exit 0 within {EXIT_SECONDS:g} s of stdin closing (status {status})"
    return (f"ready, turn_started, progress_delta×{sum(e['type'] == 'progress_delta' for e in events)}, "
            f"text_delta×{sum(e['type'] == 'text_delta' for e in events)}, context_usage {used}/{MODEL_LIMIT}, "
            f"turn_finished; {len(ctx.catalog) + 1} tools and every field as specified; exit 0 on stdin EOF")


def case_settings(ctx):
    stub = ctx.stub()
    first = Reply("msg_settings_1", usage(800)).text(["First."]).end("end_turn", 2)
    stub.script(first, Reply("msg_settings_2", usage(820)).text(["Second."]).end("end_turn", 2))
    app = ctx.app(stub)
    app.send("One.")
    check_turn(app.turn(TURN_SECONDS))
    app.update_settings({"auto_compact_tokens": None, "clear_tool_results": True})
    app.send("Two.")
    check_turn(app.turn(TURN_SECONDS))
    posts = stub.wait_posts(2, 5.0)
    assert len(posts) == 2, f"{len(posts)} requests for two turns"
    check_request(posts[0], ctx.catalog, clearing=False)
    body = check_request(posts[1], ctx.catalog, clearing=True)
    messages = posts[1].messages()
    assert len(messages) == 3, f"the second request has {len(messages)} messages"
    assert messages[0] == posts[0].messages()[0], "the first user message changed bytes"
    assert json.loads(messages[1]).get("role") == "assistant", f"message 1 is {messages[1][:120]!r}"
    check_exact(raw_members(messages[1])["content"], accumulate(first.events)[0]["content"],
                "the second request's assistant turn")
    assert body["messages"][2] == user("Two."), f"the new user message is {body['messages'][2]!r}"
    return "the turn after a settings message carries context_management and its beta; the first did not"


def case_tool_round_trip(ctx):
    stub = ctx.stub()
    deferred = deferred_tool(ctx.catalog)
    first, second = tool_round_trip_replies(deferred)
    third = Reply("msg_tools_3", usage(120, cache_read=6100)).text(["You're welcome."]).end("end_turn", 5)
    stub.script(first, second, third)
    app = ctx.app(stub)
    app.send(BLUR_REQUEST)
    events = []
    requests = [app.until("tool_request", TURN_SECONDS, events)]
    try:
        requests.append(app.until("tool_request", HOLD_SECONDS, events))
    except AssertionError as error:
        raise AssertionError("the second tool_request did not come while the first was unanswered: the helper "
                             f"must ask for every tool_use of a response at once ({error})") from None
    answers = tool_answers(deferred)
    for request in requests:  # in arrival order, as the app's main-queue hops answer them
        app.answer(request, answers[request["name"]])
    events += app.turn(TURN_SECONDS)
    check_turn(events)
    inputs = {deferred: {"filter": "gaussian_blur", "radius": 2.5, "document_id": 1}, "render": {"document_id": 1}}
    assert [r["name"] for r in requests] == [deferred, "render"], f"tool_requests {[r['name'] for r in requests]}"
    assert len({r["call_id"] for r in requests}) == 2, "two tool_requests share a call_id"
    for request in requests:
        assert json.loads(request["arguments_json"]) == inputs[request["name"]], \
            f"{request['name']} arguments_json {request['arguments_json']!r}"
        position = events.index(request)
        calls = [e for e in events[:position] if e["type"] == "tool_call" and e.get("name") == request["name"]]
        assert calls and calls[-1].get("arguments") == inputs[request["name"]], \
            f"no tool_call with its arguments before the {request['name']} tool_request"
    shown = [(e["name"], e["is_error"], e["summary"]) for e in events if e["type"] == "tool_result"]
    assert shown == [(deferred, True, FILTER_REFUSAL[:200]), ("render", False, "(image)")], \
        f"tool_result events {shown}"
    progress, text = texts(events, "progress_delta"), texts(events, "text_delta")
    assert progress == "I'll blur the background, then look at it.", f"progress_delta text {progress!r}"
    assert text == "Blurring now.Done — the background is blurred.", f"text_delta text {text!r}"
    used = [e["used"] for e in events if e["type"] == "context_usage"]
    want = [context_tokens(accumulate(reply.events)[0]["usage"]) for reply in (first, second)]
    assert want[0] in used and want[1] in used[used.index(want[0]) + 1:], \
        f"context_usage {used}, expected {want} in order"

    app.send("Thanks!")
    check_turn(app.turn(TURN_SECONDS))
    posts = stub.wait_posts(3, 5.0)
    assert len(posts) == 3, f"{len(posts)} requests, expected 3"
    bodies = [check_request(post, ctx.catalog) for post in posts]
    assert bodies[0]["messages"] == [user(BLUR_REQUEST)], f"the first request's messages {bodies[0]['messages']!r}"
    members = [post.members() for post in posts]
    for later in (1, 2):
        assert list(members[later]) == list(members[0]), f"request {later + 1}'s field order differs from request 1's"
        for field, raw in members[0].items():
            if field != "messages":
                assert members[later][field] == raw, (f"request {later + 1}'s {field} is not byte-identical "
                                                      f"to request 1's: {first_difference(members[later][field], raw)}")
    messages = [post.messages() for post in posts]
    assert len(messages[1]) == 3, f"the second request has {len(messages[1])} messages, expected 3"
    assert messages[1][0] == messages[0][0], "the first user message changed bytes"
    assistant = load_exact(messages[1][1])
    assert list(assistant) in (["role", "content"], ["content", "role"]) and assistant["role"] == "assistant", \
        f"the assistant turn's fields {list(assistant)}"
    check_exact(raw_members(messages[1][1])["content"], accumulate(first.events)[0]["content"],
                "the second request's assistant turn")
    results = json.loads(messages[1][2])
    assert without_false_is_error(results) == expected_tool_results(), \
        "the tool results are not the answers in tool_use order (text, isError, image/png, dropped items)"
    assert len(messages[2]) == 5, f"the third request has {len(messages[2])} messages, expected 5"
    for index in range(3):
        assert messages[2][index] == messages[1][index], (
            f"message {index} changed bytes between requests 2 and 3: "
            f"{first_difference(messages[2][index], messages[1][index])}")
    check_exact(raw_members(messages[2][3])["content"], accumulate(second.events)[0]["content"],
                "the third request's second assistant turn")
    assert json.loads(messages[2][4]) == user("Thanks!"), f"the last message {messages[2][4][:200]!r}"
    betas = {post.header("anthropic-beta") for post in posts}
    assert len(betas) == 1, f"the beta header changed between requests: {betas}"
    return ("both tool_requests before any answer; request 2 replays the reply verbatim (thinking signatures, "
            "tool search blocks, key order, 2.50) with the results in tool_use order; tools, system and the "
            f"prefix byte-identical across 3 requests ({len(posts[1].body) // 1024} KB)")


def case_cancel_stream(ctx):
    stub = ctx.stub()
    partial = ["Once up", "on a mid", "night dreary"]
    stub.script(Reply("msg_cancel_1", usage(900)).text(partial, close=False).stall(),
                Reply("msg_cancel_2", usage(950)).text(["A short one."]).end("end_turn", 5))
    app = ctx.app(stub)
    app.send("Tell me a long story.")
    events = []
    while sum(e["type"] == "text_delta" for e in events) < len(partial):
        app.until("text_delta", TURN_SECONDS, events)
    cancelled = time.monotonic()
    app.cancel()
    events += app.turn(CANCEL_SECONDS)
    finished = time.monotonic() - cancelled
    check_turn(events)
    posts = stub.posts()
    assert posts[0].disconnected.wait(10.0), "the stalled request's connection was still open 10 s after cancel"
    app.helper.drain(QUIET_SECONDS)
    assert len(stub.posts()) == 1, "the helper sent another request after the cancel"
    app.send("Shorter, please.")
    second = app.turn(TURN_SECONDS)
    check_turn(second)
    assert texts(second, "text_delta") == "A short one.", f"text_delta {texts(second, 'text_delta')!r}"
    posts = stub.wait_posts(2, 5.0)
    body = check_request(posts[1], ctx.catalog)
    for piece in partial + ["".join(partial)]:
        assert piece not in posts[1].text, f"the next request holds the cancelled reply's {piece!r}"
    assert body["messages"] == [user("Tell me a long story.", "Shorter, please.")], \
        f"the next request's messages {body['messages']!r} (expected the merged user message)"
    return (f"turn_finished {finished:.2f} s after cancel with no error; the connection closed; no request "
            "followed; the next request merged both texts into one user message and held no reply")


def case_cancel_tools(ctx):
    stub = ctx.stub()
    first = (Reply("msg_ct_1", usage(1500)).thinking(["Rendering first."], SIGNATURES["other"])
             .tool_use("toolu_01R", "render", ['{"document_id": 1}']).end("tool_use", 60))
    stub.script(first, Reply("msg_ct_2", usage(1600)).text(["OK."]).end("end_turn", 3))
    app = ctx.app(stub)
    app.send("Render the canvas.")
    events = []
    request = app.until("tool_request", TURN_SECONDS, events)
    app.cancel()  # Stop while the call waits for main ...
    app.answer(request, RENDER_RESULT)  # ... so runTool answers "Cancelled by the user."
    events += app.turn(CANCEL_SECONDS)
    check_turn(events)
    shown = [(e["name"], e["is_error"], e["summary"]) for e in events if e["type"] == "tool_result"]
    assert shown == [("render", True, "Cancelled by the user.")], f"tool_result events {shown}"
    app.helper.drain(QUIET_SECONDS)
    assert len(stub.posts()) == 1, "the helper sent a request after the cancel"
    app.send("Never mind; just say OK.")
    check_turn(app.turn(TURN_SECONDS))
    posts = stub.wait_posts(2, 5.0)
    check_request(posts[1], ctx.catalog)
    messages = posts[1].messages()
    assert len(messages) == 3, f"the next request has {len(messages)} messages, expected 3"
    assert messages[0] == posts[0].messages()[0], "the first user message changed bytes"
    check_exact(raw_members(messages[1])["content"], accumulate(first.events)[0]["content"], "the assistant turn")
    expected = {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "toolu_01R",
         "content": [{"type": "text", "text": "Cancelled by the user."}], "is_error": True},
        {"type": "text", "text": "Never mind; just say OK."}]}
    got = json.loads(messages[2])
    assert got == expected, \
        f"the next request's last message {got!r}: every tool_use needs its tool_result, then the text"
    return "the waiting call answered cancelled became a tool_result; no request followed; the next send merged into it"


def case_stop_error(ctx, stop_reason, error):
    stub = ctx.stub()
    stub.script(Reply(f"msg_{stop_reason}", usage(700)).text(["This reply ends ", "here"]).end(stop_reason, 9))
    app = ctx.app(stub)
    app.send("Do the thing.")
    events = app.turn(TURN_SECONDS)
    check_turn(events, errors=[error])
    return f"stop_reason {stop_reason}: error {error!r}, then turn_finished"


def case_refusal(ctx):
    return case_stop_error(ctx, "refusal", REFUSAL_ERROR)


def case_max_tokens(ctx):
    return case_stop_error(ctx, "max_tokens", MAX_TOKENS_ERROR)


def case_context_window(ctx):
    return case_stop_error(ctx, "model_context_window_exceeded", CONTEXT_WINDOW_ERROR)


RATE_MESSAGE = "Number of request tokens has exceeded your per-minute rate limit"


def case_rate_limit_retry(ctx):
    stub = ctx.stub()
    stub.script(Status(429, "rate_limit_error", RATE_MESSAGE, {"retry-after": "0"}),
                Reply("msg_after_retry", usage(800)).text(["After the retry."]).end("end_turn", 4))
    app = ctx.app(stub)
    app.send("Hello again.")
    events = app.turn(TURN_SECONDS)
    check_turn(events)
    assert texts(events, "text_delta") == "After the retry.", f"text_delta {texts(events, 'text_delta')!r}"
    app.helper.drain(QUIET_SECONDS)
    posts = stub.posts()
    assert len(posts) == 2, f"the stub saw {len(posts)} requests, expected the first and one retry"
    assert posts[1].body == posts[0].body, f"the retry's body differs: {first_difference(posts[1].text, posts[0].text)}"
    gap = posts[1].received - posts[0].received
    return f"429 with retry-after: 0, one retry {gap:.2f} s later with the same body, no error event"


def case_retries_exhausted(ctx):
    stub = ctx.stub()
    stub.script(*[Status(429, "rate_limit_error", RATE_MESSAGE, {"retry-after-ms": "1"}) for _ in range(5)])
    app = ctx.app(stub)
    app.send("Hello again.")
    events = app.turn(TURN_SECONDS)
    check_turn(events, errors=[f"API error 429: {RATE_MESSAGE}"])
    app.helper.drain(QUIET_SECONDS)
    posts = stub.posts()
    assert len(posts) == 4, f"the stub saw {len(posts)} requests; maxRetries 3 is 1 + 3"
    return "four 429s: 1 + 3 attempts, then 'API error 429: …'"


def case_stream_error(ctx):
    stub = ctx.stub()
    stub.script(Reply("msg_overloaded", usage(600)).text(["Partial ans", "wer"], close=False)
                .error("overloaded_error", "Overloaded"),
                Reply("msg_recovered", usage(650)).text(["Recovered."]).end("end_turn", 3))
    app = ctx.app(stub)
    app.send("Try something.")
    events = app.turn(TURN_SECONDS)
    check_turn(events, errors=["API error 529: Overloaded"])
    app.helper.drain(1.5)
    assert len(stub.posts()) == 1, "a mid-stream error event was retried"
    app.send("Try again.")
    second = app.turn(TURN_SECONDS)
    check_turn(second)
    posts = stub.wait_posts(2, 5.0)
    body = check_request(posts[1], ctx.catalog)
    assert "Partial ans" not in posts[1].text, "the next request holds the failed reply's text"
    assert body["messages"] == [user("Try something.", "Try again.")], \
        f"the next request's messages {body['messages']!r}"
    return "'API error 529: Overloaded', not retried; the next request merged the texts and held no partial reply"


def _reverse_keys(value):
    if isinstance(value, dict):
        return {key: _reverse_keys(value[key]) for key in reversed(list(value))}
    if isinstance(value, list):
        return [_reverse_keys(item) for item in value]
    return value


def case_two_processes(ctx):
    stub = ctx.stub()
    deferred = deferred_tool(ctx.catalog)
    line = configure_line(stub.base_url, ctx.catalog)
    for label in ("first", "second"):
        stub.script(*tool_round_trip_replies(deferred))
        app = ctx.app(stub, label=label, line=line)
        app.send(BLUR_REQUEST)
        check_turn(app.turn(TURN_SECONDS, answer=lambda request: tool_answers(deferred)[request["name"]]))
        app.helper.end()
    posts = stub.wait_posts(4, 5.0)
    assert len(posts) == 4, f"{len(posts)} requests for two two-request conversations"
    assert posts[2].body == posts[0].body, \
        f"the second process's first request differs: {first_difference(posts[2].text, posts[0].text)}"
    second = "byte-identical" if posts[3].body == posts[1].body else \
        f"different ({first_difference(posts[3].text, posts[1].text)})"
    ctx.notes.append(f"the second requests (with the tool results) are {second}")
    stub.script(Reply("msg_reordered", usage(500)).text(["Hi."]).end("end_turn", 2))
    reordered = json.dumps(_reverse_keys(json.loads(line)), ensure_ascii=False, separators=(",", ":"))
    app = ctx.app(stub, label="reordered", line=reordered)
    app.send(BLUR_REQUEST)
    check_turn(app.turn(TURN_SECONDS))
    posts = stub.wait_posts(5, 5.0)
    same = posts[4].body == posts[0].body
    ctx.notes.append("a configure with every object's keys reversed gives " +
                     ("the same first request bytes" if same else
                      "different first request bytes: the helper keeps the app's key order"))
    return f"two processes, the same configure and conversation: identical first requests ({len(posts[0].body)} bytes)"


def case_framing(ctx):
    stub = ctx.stub()
    stub.script(Reply("msg_busy", usage(500)).text(["Working"], close=False).stall())
    helper = ctx.helper()
    app = App(helper)
    helper.write_line("")
    helper.write_line("this is not json")
    helper.write({"type": "send", "text": "Too early."})  # the app never sends before configure
    bad, early = helper.next(EVENT_SECONDS), helper.next(EVENT_SECONDS)
    assert bad.get("type") == "error" and str(bad.get("message", "")).startswith(BAD_MESSAGE_PREFIX), \
        f"the bad line got {bad!r}"
    assert early == {"type": "error", "message": NOT_CONFIGURED_ERROR}, f"the early send got {early!r}"
    helper.write({"type": "reset"})  # a type the helper no longer knows is ignored
    late = helper.drain(0.5)
    assert not late, f"events after the early send and a reset: {brief(late)}"
    helper.write_line(configure_line(stub.base_url, ctx.catalog))
    helper.write({"type": "send"})  # a send without its text
    events = []
    missing = app.until("error", EVENT_SECONDS, events)
    assert str(missing.get("message", "")).startswith(BAD_MESSAGE_PREFIX), f"a send without text got {missing!r}"
    assert "turn_started" not in [e["type"] for e in events], f"a send without text started a turn: {brief(events)}"
    app.send("Start a long reply.")
    events = []
    app.until("text_delta", TURN_SECONDS, events)
    helper.write({"type": "send", "text": "A second message."})  # the app never sends while busy
    busy = app.until("error", EVENT_SECONDS, events)
    assert busy.get("message") == BUSY_ERROR, f"a send during a turn got {busy!r}"
    app.cancel()
    events += app.turn(CANCEL_SECONDS)
    check_turn(events, errors=[BUSY_ERROR])
    return ("blank line skipped, a bad line and a send without text answered, reset ignored, unconfigured and "
            "busy sends refused without turn events")


CASES = [("text-turn", case_text_turn), ("settings", case_settings), ("tool-round-trip", case_tool_round_trip),
         ("cancel-stream", case_cancel_stream), ("cancel-tools", case_cancel_tools), ("refusal", case_refusal),
         ("max-tokens", case_max_tokens), ("context-window", case_context_window),
         ("rate-limit-retry", case_rate_limit_retry),
         ("retries-exhausted", case_retries_exhausted), ("stream-error", case_stream_error),
         ("two-processes", case_two_processes), ("framing", case_framing)]


# ---- The tool catalog the panel configures.

SYNTHETIC_CATALOG = [
    {"name": "list_documents",
     "description": "Lists the images open in Doctored with their ids, sizes, and layer counts.",
     "inputSchema": {"type": "object", "properties": {}, "required": []}},
    {"name": "get_document", "description": "Reports one document's layers: kind, depth and children.",
     "inputSchema": {"type": "object", "properties": {"document_id": {"type": "integer"}},
                     "required": ["document_id"]}},
    {"name": "render", "description": "Renders the flattened canvas as a PNG — scale×scale blocks for pixel art.",
     "inputSchema": {"type": "object", "properties": {"document_id": {"type": "integer"},
                                                      "scale": {"type": "integer", "minimum": 1}},
                     "required": ["document_id"]}},
    {"name": "open_document", "description": "Opens an image file.",
     "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
    {"name": "apply_filter", "description": "Applies a filter (gaussian_blur, sharpen, …) to the active layer.",
     "inputSchema": {"type": "object", "properties": {
         "document_id": {"type": "integer"}, "filter": {"type": "string", "enum": ["gaussian_blur", "sharpen"]},
         "radius": {"type": "number", "minimum": 0.1}}, "required": ["document_id", "filter"]}},
    {"name": "add_layer", "description": "Adds an empty layer above the active one.",
     "inputSchema": {"type": "object", "properties": {"document_id": {"type": "integer"}, "name": {"type": "string"}},
                     "required": ["document_id"]}},
]


def load_catalog(helper, explicit):
    path = Path(explicit) if explicit else helper.parent.parent / "Resources" / Path(CATALOG_IN_BUNDLE).name
    if explicit or (helper.parent.name == "Helpers" and path.is_file()):
        data = json.loads(path.read_text())
        entries = data["tools"] if isinstance(data, dict) else data
        tools = [entry for entry in entries if entry.get("name") not in PANEL_EXCLUDED_TOOLS]
        assert "render" in [entry["name"] for entry in tools], f"{path} has no render tool"
        return tools, f"{path} minus close_document ({len(tools)} tools)"
    return SYNTHETIC_CATALOG, (f"the built-in catalog ({len(SYNTHETIC_CATALOG)} tools; no {CATALOG_IN_BUNDLE} "
                               "beside the helper)")


def default_helper():
    bundle = app_under_test.default_app_bundle()
    if bundle.suffix != ".app":  # DR_VERIFY_APP named the executable
        bundle = bundle.resolve().parents[2]
    return bundle / HELPER_IN_BUNDLE


def require_fresh(helper):
    if os.environ.get("DR_ALLOW_STALE_APP") == "1":
        return
    agent = ROOT / "agent"
    inputs = [agent / "Package.swift", agent / "Package.resolved", *(agent / "Sources").rglob("*.swift")]
    built = helper.stat().st_mtime_ns
    stale = [path for path in inputs if path.is_file() and path.stat().st_mtime_ns > built]
    if stale:
        newest = max(stale, key=lambda path: path.stat().st_mtime_ns)
        raise SystemExit(f"refusing to drive {helper}: older than {newest.relative_to(ROOT)}. Rebuild it "
                         "(`make app-debug`, `make app` for Release, `make agent` for build/agent/doctored-agent), "
                         "or set DR_ALLOW_STALE_APP=1.")


def describe(error):
    return str(error) if isinstance(error, AssertionError) else f"{type(error).__name__}: {error}"


def run(helper, catalog_path, selected, keep):
    if not (helper.is_file() and os.access(helper, os.X_OK)):
        print(f"FAIL helper: {helper} is not an executable file (build it with `make app-debug`, or `make agent` "
              "and pass --helper build/agent/doctored-agent)")
        return 1
    require_fresh(helper)
    catalog, source = load_catalog(helper, catalog_path)
    print(f"helper: {helper}\ncatalog: {source}", flush=True)
    folder = Path(tempfile.mkdtemp(prefix="dr-agent-helper-"))
    failures, passed = [], 0
    try:
        for name, body in CASES:
            if selected and name not in selected:
                continue
            ctx = Context(helper, catalog, folder / name)
            try:
                detail = body(ctx)
                notes = ctx.audit()
                passed += 1
                print(f"PASS {name}: {detail}", flush=True)
                for note in ctx.notes + notes:
                    print(f"  note: {note}", flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports, then the run fails
                failures.append(name)
                print(f"FAIL {name}: {describe(error)}", flush=True)
                for child in ctx.helpers:
                    tail = child.stderr_tail()
                    if tail:
                        print(f"  stderr of {child.label}: {tail}", flush=True)
            finally:
                ctx.cleanup()
    finally:
        if keep:
            print(f"kept: {folder}", flush=True)
        else:
            shutil.rmtree(folder, ignore_errors=True)
    print(f"verify_agent_helper: {passed} passed, {len(failures)} failed" +
          (f" ({', '.join(failures)})" if failures else ""), flush=True)
    return 1 if failures else 0


# ---- Self-test: the stub, the oracle and the child wrapper, with no helper.

def _iter_sse(response):
    name, data = None, []
    while True:
        line = response.readline()
        if not line:
            return
        line = line.decode("utf-8").rstrip("\r\n")
        if not line:
            if name is not None or data:
                yield name, "\n".join(data)
            name, data = None, []
            continue
        if line.startswith(":"):
            continue
        field, _, value = line.partition(":")
        value = value[1:] if value.startswith(" ") else value
        if field == "event":
            name = value
        elif field == "data":
            data.append(value)


def _post(connection, body=b'{"model":"x"}'):
    connection.request("POST", "/v1/messages", body=body,
                       headers={"Content-Type": "application/json", "Authorization": f"Bearer {DUMMY_KEY}"})
    return connection.getresponse()


def self_test():
    failures = []

    def check(name, body):
        try:
            print(f"PASS {name}: {body()}", flush=True)
        except Exception as error:  # noqa: BLE001
            failures.append(name)
            print(f"FAIL {name}: {describe(error)}", flush=True)

    def json_helpers():
        text = '{"b": [1, 2.50], "a" : {"y":1,"x":2}, "s":"—\\u00d7"}'
        members = raw_members(text)
        assert list(members) == ["b", "a", "s"], list(members)
        assert members == {"b": "[1, 2.50]", "a": '{"y":1,"x":2}', "s": '"—\\u00d7"'}, members
        assert raw_members(' [ {"k": 1} , 2 ] ') == ['{"k": 1}', "2"]
        assert dump_exact(load_exact('{"r": 2.50, "a": 1}')) == '{"r":2.50,"a":1}'
        assert dump_exact(load_exact('{"a":1,"b":2}')) != dump_exact(load_exact('{"b":2,"a":1}'))
        assert dump_exact(load_exact('{"s":"\\u00d7"}')) == dump_exact(load_exact('{"s":"×"}'))
        assert len(FILTER_REFUSAL) > 200 and FILTER_REFUSAL.isascii()
        return "raw member spans, key order and number text kept, escapes normalised"

    def oracle():
        first, second = tool_round_trip_replies("apply_filter")
        message, complete = accumulate(first.events)
        expected = ('[{"type":"thinking","thinking":"","signature":"' + SIGNATURES["reasoning"] + '"},'
                    '{"type":"server_tool_use","id":"srvtoolu_01S","name":"tool_search_tool_bm25",'
                    '"input":{"query":"blur filter"}},'
                    '{"type":"tool_search_tool_result","tool_use_id":"srvtoolu_01S","content":{"type":'
                    '"tool_search_tool_search_result","tool_references":[{"type":"tool_reference",'
                    '"tool_name":"apply_filter"}]}},'
                    '{"type":"thinking","thinking":"I\'ll blur the background, then look at it.","signature":"'
                    + SIGNATURES["progress"] + '"},{"type":"text","text":"Blurring now."},'
                    '{"type":"tool_use","id":"toolu_01A","name":"apply_filter","input":{"filter":"gaussian_blur",'
                    '"radius":2.50,"document_id":1},"caller":{"type":"direct"}},'
                    '{"type":"tool_use","id":"toolu_01B","name":"render","input":{"document_id":1}}]')
        got = dump_exact(message["content"])
        assert got == expected, first_difference(got, expected)
        assert complete and message["stop_reason"] == "tool_use", message["stop_reason"]
        assert context_tokens(message["usage"]) == 2048 + 1800 + 187, message["usage"]
        partial, complete = accumulate(Reply("m", usage(5)).text(["a", "b"], close=False).stall().events)
        assert not complete and dump_exact(partial["content"]) == '[{"type":"text","text":"ab"}]'
        second_message, _ = accumulate(second.events)
        assert dump_exact(second_message["content"]) == '[{"type":"text","text":"Done — the background is blurred."}]'
        return "a scripted reply accumulates to the hand-written blocks (order, 2.50, signatures, usage)"

    def stub_stream_and_models():
        stub = StubAPI()
        try:
            first, _ = tool_round_trip_replies("apply_filter")
            stub.script(first)
            connection = http.client.HTTPConnection("127.0.0.1", stub.server.server_address[1], timeout=10)
            body = '{"model":"claude-opus-5-5","messages":[{"role":"user","content":"é"}]}'.encode()
            response = _post(connection, body)
            assert response.status == 200 and response.getheader("Content-Type").startswith("text/event-stream")
            events = list(_iter_sse(response))
            assert events == first.events, "the streamed events are not the script's, byte for byte"
            connection.request("GET", f"/v1/models/{MODEL}")  # the same kept-alive connection
            info = json.loads(connection.getresponse().read())
            assert info["id"] == MODEL and info["max_input_tokens"] == MODEL_LIMIT, info
            posts = stub.posts()
            assert len(posts) == 1 and posts[0].body == body
            assert posts[0].header("authorization") == f"Bearer {DUMMY_KEY}", posts[0].headers
            return (f"{len(events)} SSE events as scripted, the body recorded byte for byte, models on a kept-alive "
                    "connection")
        finally:
            stub.close()

    def stub_status_then_stream():
        stub = StubAPI()
        try:
            stub.script(Status(429, "rate_limit_error", RATE_MESSAGE, {"retry-after": "0"}),
                        Reply("m", usage(5)).text(["ok"]).end("end_turn", 1))
            connection = http.client.HTTPConnection("127.0.0.1", stub.server.server_address[1], timeout=10)
            response = _post(connection)
            error = json.loads(response.read())
            assert response.status == 429 and response.getheader("retry-after") == "0", response.status
            assert error["error"] == {"type": "rate_limit_error", "message": RATE_MESSAGE}, error
            events = list(_iter_sse(_post(connection)))
            assert events[-1] == ("message_stop", '{"type":"message_stop"}'), events[-1]
            return "429 with retry-after and the API's error body, then a whole stream on the same connection"
        finally:
            stub.close()

    def stub_stall():
        stub = StubAPI()
        try:
            stub.script(Reply("m", usage(5)).text(["a", "b", "c"], close=False).stall())
            connection = http.client.HTTPConnection("127.0.0.1", stub.server.server_address[1], timeout=10)
            stream = _iter_sse(_post(connection))
            deltas = 0
            while deltas < 3:
                name, _ = next(stream)
                deltas += name == "content_block_delta"
            started = time.monotonic()
            connection.sock.shutdown(socket.SHUT_RDWR)
            connection.close()
            record = stub.posts()[0]
            assert record.disconnected.wait(5.0), "the stub never saw the client go"
            return f"held open after 3 deltas; the client's close seen in {time.monotonic() - started:.2f} s"
        finally:
            stub.close()

    def stub_stream_error_and_unscripted():
        stub = StubAPI()
        try:
            stub.script(Reply("m", usage(5)).text(["Partial"], close=False).error("overloaded_error", "Overloaded"))
            connection = http.client.HTTPConnection("127.0.0.1", stub.server.server_address[1], timeout=10)
            events = list(_iter_sse(_post(connection)))
            assert events[-1][0] == "error" and json.loads(events[-1][1])["error"]["type"] == "overloaded_error"
            assert not any(name == "message_stop" for name, _ in events)
            response = _post(connection)
            refused = json.loads(response.read())
            assert response.status == 400 and refused["error"]["type"] == "invalid_request_error", refused
            assert len(stub.problems) == 1, stub.problems
            return "an error event ends the body; an unscripted request is a 400 and a recorded problem"
        finally:
            stub.close()

    def child_wrapper():
        folder = Path(tempfile.mkdtemp(prefix="dr-agent-helper-self-"))
        child = Helper(Path("/bin/cat"), folder, "cat")
        try:
            child.write({"type": "ready"})
            assert child.next(EVENT_SECONDS) == {"type": "ready"}
            child.write_line("not protocol")
            big = {"type": "tool_result", "call_id": "1", "result": json.loads(RENDER_RESULT)}
            child.write(big)
            assert child.next(EVENT_SECONDS) == big and child.violations == [b"not protocol\n"], child.violations
            try:
                child.next(0.3)
                raise AssertionError("silence was not reported")
            except Silent:
                pass
            status = child.close_stdin_and_wait(EXIT_SECONDS)
            assert status == 0, status
            return f"events and violations told apart, a {len(RENDER_RESULT) // 1024} KB line round-trips, exit on EOF"
        finally:
            child.end()
            shutil.rmtree(folder, ignore_errors=True)

    for name, body in [("json helpers", json_helpers), ("oracle", oracle),
                       ("stub stream and models", stub_stream_and_models),
                       ("stub status then stream", stub_status_then_stream), ("stub stall", stub_stall),
                       ("stub stream error and unscripted", stub_stream_error_and_unscripted),
                       ("child wrapper", child_wrapper)]:
        check(name, body)
    print(f"verify_agent_helper --self-test: {7 - len(failures)} passed, {len(failures)} failed", flush=True)
    return 1 if failures else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--helper", type=Path, help="the built doctored-agent (default: the Debug bundle's)")
    parser.add_argument("--catalog", help="a tool catalog JSON (default: the bundle's agent-catalog.json)")
    parser.add_argument("--cases", help="comma-separated case names: " + ", ".join(name for name, _ in CASES))
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder (helper stderr logs)")
    parser.add_argument("--self-test", action="store_true", help="check the stub, oracle and child wrapper only")
    arguments = parser.parse_args()
    if arguments.self_test:
        raise SystemExit(self_test())
    selected = set(filter(None, (arguments.cases or "").split(",")))
    unknown = selected - {name for name, _ in CASES}
    if unknown:
        parser.error(f"unknown cases {sorted(unknown)}")
    helper = (arguments.helper or default_helper()).expanduser().absolute()
    raise SystemExit(run(helper, arguments.catalog, selected, arguments.keep))


if __name__ == "__main__":
    main()
