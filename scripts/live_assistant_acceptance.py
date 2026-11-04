#!/usr/bin/env python3
"""The assistant helper's live acceptance run: `Contents/Helpers/doctored-agent` against the
Claude API, with an owned Doctored running every tool call.

The run plays the app's side of the helper protocol exactly as `AssistantHelperSession` and
`AssistantPanelViewController` do, but between the helper and the API it puts a recording proxy,
and every tool the model calls runs on a real, owned Doctored instance.

Modes (one is required):
- `--self-test`: the upstream is scripts/verify_agent_helper.py's loopback `StubAPI`, scripted to
  answer every scenario; the key is a dummy one written to a temp file and read back through the
  same key-file path the live mode uses. Nothing leaves the machine.
- `--live --yes-spend [--budget-usd 5.00] [--key-file PATH]`: the upstream is
  https://api.anthropic.com and the key is read from --key-file (default the app's own
  ~/Library/Application Support/Doctored/anthropic_api_key, read as `SecretFile.read` does: UTF-8,
  trimmed). The key stays inside this process and goes to the helper only in `configure`, over
  its stdin. Without --yes-spend the live mode refuses; it never prompts.

The pieces:
- The helper is the app bundle's (`--app`, default the worktree's Release bundle), run under the
  app's scrubbed environment (HOME, TMPDIR, PATH=/usr/bin:/bin) by verify_agent_helper's `Helper`.
  `configure` is built as the panel builds it: `model` is ensureSession's default,
  `system` is `systemPrompt()` assembled from its Swift literals with the opened document's facts
  (the `\\(...)` interpolations are filled; an unknown one stops the run), `core_tools`,
  `search_guide` and `compaction_instructions` are `AssistantHelperPrompt`'s literals, `tools` is
  the bundle's Contents/Resources/agent-catalog.json minus `panelExcludedTools`, spliced byte for
  byte (that file is `JSONSerialization` output of the same parsed catalog the panel sends), and
  `settings` is `AssistantChatSettings.helperJSON` of the defaults, with per-scenario overrides.
  Every message is written with sorted keys and `JSONSerialization`'s escaping (`/` as `\\/`),
  as `AssistantHelperSession.write` does; a startup note says whether that emulation reproduces
  the bundle catalog's bytes.
- The recording proxy is a ThreadingHTTPServer on 127.0.0.1, the helper's `api_base`. It forwards
  each request with its headers minus the hop-by-hop ones (and `Accept-Encoding: identity`, so
  the recorded bytes are readable without brotli), streams each response back chunk by chunk as
  it arrives, and records per request: index, method, path, status, the request and response
  body bytes (raw/ files), `anthropic-beta`, `anthropic-version` and the response's `request-id`,
  with usage (input, output, cache read, cache creation) and stop_reason, into
  build/live-acceptance/<UTC stamp>/requests.jsonl. It records, prints and logs no other header
  (the response's content-type is read to parse the body and dropped):
  `x-api-key` and `authorization` never leave it, and every text it writes (exceptions included)
  goes through a redactor first. A last scan of the run folder fails the run if the key is in it.
- The budget guard prices every response at Claude Opus 5.5's rates (input $4, output $20, cache
  read $0.20, 5-minute cache write $5, 1-hour cache write $8 per MTok; `usage.iterations` summed
  when it is larger, since it itemizes a compaction or fallback attempt; a cut-off stream's
  output is estimated from the text it carried). Once the estimate passes --budget-usd the turn
  is cancelled (before any tool of the reply that passed it runs), every later POST is refused by
  the proxy itself (a local 400, never forwarded), and the remaining scenarios are skipped. The
  check follows each response, so the response that passes the budget overshoots it by its own
  cost (at most about $1: a 32000-token reply on a large context).
- The owned instance is the root CLAUDE.md recipe (`verification_isolation.owned_environment`:
  scratch DR_AGENT_SOCKET, DR_BACKGROUND=1, ANTHROPIC_API_KEY=dummy for the APP, private folders,
  DR_WORKING_LIMIT_BYTES, DR_AUTOMATION_ALERTS=record), checked by `require_fresh_app`; it opens
  --image (default samples/shapes-alpha.png, 400x400 RGB, a 10 KB render). Each tool_request is
  an MCP tools/call over the socket, answered with the CallToolResult as `runTool` answers
  (semantically equal: the MCP server re-serializes `execute`'s JSON), and "Cancelled by the
  user." once a cancel was sent in that turn. The instance is terminated and its socket removed
  in `finally`; only children this run started are signalled.

Scenarios, each a fresh helper process, each PASS/FAIL with its evidence:
- S1: three turns on the document (describe it; add an empty layer named Notes and report the
  count; undo it): every request 200, no error event, turn_finished each turn, tool calls ran on
  the instance (its layers checked after turns 2 and 3), cache reads on every request after the
  first, and each turn's first request reading more cache than the previous turn's.
- S2: a new process with S1's configure sends S1's first message: its first request reads cache,
  and its tools and system bytes equal S1's first request's.
- S3: a cancel after 15 text deltas of an essay: turn_finished, no error, the stream cut off at
  the proxy and no request after it; then "Reply with just OK." gets a 200 and a reply that holds
  no part of the cancelled one.
- S4: a cancel at the first tool_request, answered as the app does: turn_finished, no error, no
  tool ran; then "Reply with just OK." gets a 200 (no 400 about an unanswered tool_use).
- S5: eight turns mixing tools and reasoning: every request 200, no error event, no 400 naming
  thinking, prefix or tool_use; total requests and the cache hit ratio.
- S6: auto_compact_tokens at the app's minimum in a conversation past it: compaction_started
  then compacted (never compaction_failed), and the next turn's request, led by the summary, 200.
Refusal and max_tokens are not attempted live (no prompting for harmful content; max_tokens is
fixed at 32000): verify_agent_helper.py's refusal and max-tokens cases and `make agent-test` cover
them. Self-test adds a budget-guard check against a second stub.

Output: a PASS/FAIL line per scenario as it ends, a summary table (scenario, verdict, requests,
statuses, cache read/creation, error events, cost) and the running total on stdout, and
summary.json, requests.jsonl, raw/, app.log and helper-<scenario>.log in the run folder. Every
wait is bounded (a turn 300 s live, 30 s self-test), so a hang fails its scenario, not the run.
Exits 0 only when every selected scenario passed.

A run that fails outside a scenario (an instance that never answers) or is stopped by SIGINT,
SIGTERM or SIGHUP (one ignored at start, as under nohup, stays ignored) still ends the instance
and every helper, scans the folder for the key and writes summary.json (verdict FAIL, with
`error` or `interrupted`) before it prints the table; a stop exits 128 + the signal. A live run
takes several minutes (about 40 Messages requests near 0.9 MB each, plus the compaction turns),
so start it in the background rather than under a foreground time limit.

Usage:
  python3 scripts/live_assistant_acceptance.py --self-test
  python3 scripts/live_assistant_acceptance.py --live --yes-spend [--budget-usd 5.00]
Options: --app BUNDLE, --image PATH, --scenarios S1,S2,..., --key-file PATH (live only).
"""
from __future__ import annotations

import argparse
import collections
import copy
import datetime
import hashlib
import http.client
import http.server
import io
import json
import os
from pathlib import Path
import random
import re
import shutil
import signal
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import urllib.parse

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
import mcp_client  # noqa: E402
import verification_isolation  # noqa: E402
import verify_agent_helper as vah  # noqa: E402

ROOT = vah.ROOT
SOURCES = ROOT / "app/Sources"
RUNS = ROOT / "build/live-acceptance"
DEFAULT_APP = app_under_test.RELEASE_APP
DEFAULT_IMAGE = ROOT / "samples/shapes-alpha.png"
DEFAULT_KEY_FILE = Path.home() / "Library/Application Support/Doctored/anthropic_api_key"
LIVE_UPSTREAM = "https://api.anthropic.com"
SELF_TEST_KEY = "sk-ant-dummy-live-acceptance-self-test"

# Claude Opus 5.5, USD per million tokens. A 1-hour cache write is twice the input rate; the helper
# asks only for the 5-minute kind (top-level `cache_control: ephemeral`), so it should never apply.
RATES = {"input": 4.00, "output": 20.00, "cache_read": 0.20, "cache_write_5m": 5.00, "cache_write_1h": 8.00}

TURN_SECONDS = {"live": 300.0, "self-test": 30.0}
TOOL_SECONDS = 120.0
LAUNCH_SECONDS = 60.0
UPSTREAM_SECONDS = 600.0  # one socket read from the API; SSE pings keep a thinking stream under it
CANCEL_DRAIN_SECONDS = 30.0  # after a timed-out turn's cancel, for its turn_finished
IDLE_SECONDS = 30.0  # for the proxy to finish recording a turn's responses

SCENARIOS = ("S1", "S2", "S3", "S4", "S5", "S6")
TITLES = {"S1": "three turns", "S2": "relaunch", "S3": "cancel mid-stream", "S4": "cancel with tools pending",
          "S5": "long conversation", "S6": "compaction"}

S1_TURNS = ("What document is open right now? Describe it briefly: its size, its layers and what the image shows.",
            "Add a new empty layer named Notes, then tell me how many layers the document has now.",
            "Undo that last change and confirm the layer count is back to what it was.")
ESSAY = ("Write an essay of about 600 words on image histograms: what they show, how to read them, and how "
         "they guide exposure and tone edits. Do not call any tools.")
CANCEL_AFTER_TEXT_DELTAS = 15
RENDER_ASK = "Render the document with the render tool and describe what you see."
JUST_OK = "Reply with just OK."
S5_TURNS = ("List the layers in this document with their names, opacity and visibility.",
            "Add a new empty layer named Draft above the active layer.",
            "Rename the layer Draft to Sketch.",
            "Set the Sketch layer's opacity to 40%.",
            "Undo that opacity change, then tell me what the Sketch layer's opacity is now.",
            "Render the document and describe what you see in two sentences.",
            "Without calling any tools, think it through: which of the edits we made change how the flattened "
            "image looks, and why?",
            "Summarize what changed in this document during our conversation, as a short list.")
S6_ROUNDS = 3  # bulk turns to pass the compaction threshold before giving up
# --nonce: a run marker on each conversation's first message, so a re-run within the cache's
# 5 minutes reads nothing an earlier run left at the end of the same prompt (S2 shares S1's).
NONCE = ""
S6_NOTES_CHARACTERS = 90_000  # about 25k tokens of editing notes per bulk turn
NOT_ATTEMPTED = ("Refusal and max_tokens are not attempted live (no prompting for harmful content; max_tokens is "
                 "fixed at 32000): scripts/verify_agent_helper.py's refusal and max-tokens cases and "
                 "`make agent-test` (agent/Tests) cover them against scripted replies.")


class ScenarioError(AssertionError):
    pass


class BudgetStop(Exception):
    pass


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc)


# ---- The app's literals, read from its Swift sources.

_ESCAPES = {"\\": "\\", '"': '"', "'": "'", "n": "\n", "t": "\t", "r": "\r", "0": "\0"}


def _interpolation_end(text, opener):
    """The index just past the `)` that closes the interpolation whose `(` is at `text[opener]`."""
    depth, index, quoted = 0, opener, False
    while index < len(text):
        char = text[index]
        if quoted:
            if char == "\\":
                index += 2
                continue
            quoted = char != '"'
        elif char == '"':
            quoted = True
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    raise SystemExit("an unterminated \\( interpolation in a Swift literal")


def decode_swift(text, values, where):
    """A Swift literal's content with its escapes decoded, its line continuations (a `\\` ending a
    line) joined and each `\\(expression)` replaced by `values[expression]`."""
    out, index = [], 0
    while index < len(text):
        char = text[index]
        if char != "\\":
            out.append(char)
            index += 1
            continue
        following = text[index + 1:index + 2]
        if following == "\n":
            index += 2
        elif following == "(":
            end = _interpolation_end(text, index + 1)
            expression = text[index + 2:end - 1].strip()
            if expression not in values:
                raise SystemExit(f"{where} interpolates \\({expression}), which this script does not fill: update "
                                 "scripts/live_assistant_acceptance.py to match the app")
            out.append(values[expression])
            index = end
        elif following == "u" and text[index + 2:index + 3] == "{":
            close = text.index("}", index + 3)
            out.append(chr(int(text[index + 3:close], 16)))
            index = close + 1
        elif following in _ESCAPES:
            out.append(_ESCAPES[following])
            index += 2
        else:
            raise SystemExit(f"{where}: an escape \\{following} this script does not decode")
    return "".join(out)


def multiline_literal(source, anchor, where, values=None, start=0):
    """The value of the first `\"\"\"` literal at or after `anchor` (searched from `start`), as Swift
    builds it: each line less the closing delimiter's indentation, then `decode_swift`."""
    at = source.find(anchor, start)
    if at < 0:
        raise SystemExit(f"{where}: {anchor!r} is not in the source; update scripts/live_assistant_acceptance.py")
    opener = source.index('"""', at)
    newline = source.index("\n", opener)
    closing = re.compile(r'^([ \t]*)"""', re.M).search(source, newline + 1)
    if closing is None:
        raise SystemExit(f"{where}: the literal after {anchor!r} never closes")
    indent = closing.group(1)
    lines = source[newline + 1:closing.start()].split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    stripped = []
    for line in lines:
        if not line.strip():
            stripped.append("")
        elif line.startswith(indent):
            stripped.append(line[len(indent):])
        else:
            raise SystemExit(f"{where}: a line of the literal after {anchor!r} is indented less than its delimiter")
    return decode_swift("\n".join(stripped), values or {}, where)


def _match(pattern, source, where):
    found = re.search(pattern, source)
    if found is None:
        raise SystemExit(f"{where}: no match for {pattern!r}; update scripts/live_assistant_acceptance.py")
    return found


def _string_array(source, pattern, where):
    body = _match(pattern, source, where).group(1)
    return [decode_swift(item, {}, where) for item in re.findall(r'"((?:[^"\\]|\\.)*)"', body)]


def _tokens(text):
    return int(text.replace("_", ""))


class AppPrompt:
    """Everything the panel puts in `configure` besides the key, the tools and api_base."""

    def __init__(self):
        def read(name):
            return (SOURCES / name).read_text(encoding="utf-8")

        prompt = read("AssistantHelperPrompt.swift")
        self.core_tools = _string_array(prompt, r"static let coreTools: \[String\] = \[([^\]]*)\]", "coreTools")
        self.search_guide = multiline_literal(prompt, "static let searchGuide", "searchGuide")
        self.compaction_instructions = multiline_literal(prompt, "static let compactionInstructions",
                                                         "compactionInstructions")
        self.actions_section = multiline_literal(read("AssistantActionAuthoring.swift"),
                                                 "static let systemPromptSection", "systemPromptSection")
        self.panel = read("AssistantPanelViewController.swift")
        self.function = self.panel.find("private func systemPrompt() -> String {")
        if self.function < 0:
            raise SystemExit("AssistantPanelViewController.systemPrompt() is gone; update this script")
        self.no_document = decode_swift(_match(r'var context = "((?:[^"\\]|\\.)*)"', self.panel[self.function:],
                                               "systemPrompt").group(1), {}, "systemPrompt")
        self.model = _match(r'string\(forKey: "AssistantModel"\) \?\? "([^"]+)"', self.panel, "ensureSession").group(1)
        self.excluded = set(_string_array(read("AssistantHelperSession.swift"),
                                          r"static let panelExcludedTools: Set<String> = \[([^\]]*)\]",
                                          "panelExcludedTools"))
        settings = read("AssistantChatSettings.swift")
        for needle in ('"auto_compact_tokens": autoCompact ? autoCompactTokens as Any : NSNull()',
                       '"clear_tool_results": clearToolResults'):
            if needle not in settings:
                raise SystemExit("AssistantChatSettings.helperJSON changed; update "
                                 "scripts/live_assistant_acceptance.py")
        self.default_compact = _tokens(_match(r"static let defaultCompactTokens = ([\d_]+)", settings,
                                              "defaultCompactTokens").group(1))
        floor = _match(r"static let compactTokensRange = ([\d_]+)\.\.\.([\d_]+)", settings, "compactTokensRange")
        self.compact_floor = _tokens(floor.group(1))
        self.auto_compact = _match(r"var autoCompact = (true|false)", settings, "autoCompact").group(1) == "true"
        self.clear_tool_results = _match(r"var clearToolResults = (true|false)", settings,
                                         "clearToolResults").group(1) == "true"

    def settings(self, **overrides):
        """`AssistantChatSettings().helperJSON`, with `overrides`."""
        value = {"auto_compact_tokens": self.default_compact if self.auto_compact else None,
                 "clear_tool_results": self.clear_tool_results}
        value.update(overrides)
        return value

    def system(self, document):
        """`systemPrompt()` for `document` ({id, title, width, height, layer_count}) or for none."""
        context = self.no_document
        if document is not None:
            context = multiline_literal(self.panel, 'context = """', "systemPrompt", start=self.function, values={
                "id": str(document["id"]), 'document.displayName ?? "Untitled"': document["title"] or "Untitled",
                "doc.width": str(document["width"]), "doc.height": str(document["height"]),
                "doc.layerCount": str(document["layer_count"])})
        return multiline_literal(self.panel, 'return """', "systemPrompt", start=self.function, values={
            "context": context, "AssistantActionAuthoring.systemPromptSection": self.actions_section})


# ---- The app's JSON: JSONSerialization.data(withJSONObject:options: [.sortedKeys]) on Darwin.

class Raw:
    """JSON text `app_json` splices in as it is."""

    def __init__(self, text):
        self.text = text


def app_json(value):
    """Compact JSON as Darwin's JSONSerialization writes it with `.sortedKeys`: keys sorted (by code
    point, which is the order for every key the app writes), `/` escaped as `\\/`, non-ASCII kept
    as UTF-8, and doubles with 17 significant digits unless integral."""
    if isinstance(value, Raw):
        return value.text
    if value is None:
        return "null"
    if value is True or value is False:
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return str(int(value)) if value.is_integer() and abs(value) < 1e15 else format(value, ".17g")
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False).replace("/", "\\/")
    if isinstance(value, dict):
        return "{" + ",".join(app_json(key) + ":" + app_json(value[key]) for key in sorted(value)) + "}"
    if isinstance(value, (list, tuple)):
        return "[" + ",".join(app_json(item) for item in value) + "]"
    raise TypeError(f"app_json cannot write {type(value).__name__}")


def panel_tools(catalog_path, excluded):
    """The bundle catalog's `tools` minus `excluded`, as raw JSON text, and a note on whether
    `app_json` reproduces the file (so the other configure fields are written as the app writes them)."""
    text = catalog_path.read_text(encoding="utf-8")
    entries = vah.raw_members(vah.raw_members(text)["tools"])
    kept = [entry for entry in entries if json.loads(entry).get("name") not in excluded]
    rewritten = app_json(json.loads(text))
    note = ("app_json reproduces the bundle catalog byte for byte" if rewritten == text else
            f"app_json differs from the bundle catalog: {vah.first_difference(rewritten, text)}")
    return "[" + ",".join(kept) + "]", [json.loads(entry)["name"] for entry in kept], note


def read_key(path):
    """The key as `SecretFile.read` reads it: UTF-8, trimmed, empty meaning none."""
    try:
        text = Path(path).read_text(encoding="utf-8").strip()
    except OSError as error:
        raise SystemExit(f"could not read the API key file {path}: {type(error).__name__}") from None
    if not text:
        raise SystemExit(f"the API key file {path} is empty")
    return text


# ---- Secrets.

class Redactor:
    """Removes the key, bearer tokens and credential header values from any text this run writes."""

    PATTERNS = [(re.compile(r"sk-ant-[A-Za-z0-9_\-]{6,}"), "sk-ant-[redacted]"),
                (re.compile(r"(?i)(bearer\s+)[^\s'\",]+"), r"\1[redacted]"),
                (re.compile(r"(?i)(x-api-key|authorization)(['\"]?\s*[:=]\s*['\"]?)[^\s'\",)]+"), r"\1\2[redacted]")]

    def __init__(self, secrets):
        self.secrets = [secret for secret in secrets if secret]

    def __call__(self, text):
        text = str(text)
        for secret in self.secrets:
            text = text.replace(secret, "[redacted]")
        for pattern, replacement in self.PATTERNS:
            text = pattern.sub(replacement, text)
        return text

    def holds(self, data):
        return any(secret.encode() in data for secret in self.secrets)

    def scrub(self, data):
        for secret in self.secrets:
            data = data.replace(secret.encode(), b"[redacted]")
        return data


REDACT = Redactor([])  # main gives it the key; every line this run prints goes through it


def say(text=""):
    try:
        print(REDACT(text), flush=True)
    except OSError:
        pass  # stdout is gone (a killed caller's pipe); summary.json, written first, still holds the run


def scan_for_secrets(folder, redactor):
    """Files under `folder` that held the key; each is scrubbed in place."""
    found = []
    for path in sorted(folder.rglob("*")):
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError:
            continue
        if redactor.holds(data):
            found.append(str(path.relative_to(folder)))
            path.write_bytes(redactor.scrub(data))
    return found


# ---- Usage and cost.

def _count(usage, key):
    try:
        return int(usage.get(key) or 0)
    except (TypeError, ValueError):
        return 0


def cost_of(usage):
    """USD for one usage object (a response's, or one of its `iterations`) at RATES."""
    creation = _count(usage, "cache_creation_input_tokens")
    detail = usage.get("cache_creation")
    one_hour = min(_count(detail, "ephemeral_1h_input_tokens"), creation) if isinstance(detail, dict) else 0
    return (_count(usage, "input_tokens") * RATES["input"] + _count(usage, "output_tokens") * RATES["output"]
            + _count(usage, "cache_read_input_tokens") * RATES["cache_read"]
            + (creation - one_hour) * RATES["cache_write_5m"] + one_hour * RATES["cache_write_1h"]) / 1e6


def billed(usage):
    """The response's estimate: top-level usage covers only the attempt that produced the reply, and
    `iterations` itemizes every attempt, so the larger of the two."""
    top = cost_of(usage)
    iterations = [entry for entry in usage.get("iterations") or [] if isinstance(entry, dict)]
    return max(top, sum(cost_of(entry) for entry in iterations)) if iterations else top


def plain(value):
    return json.loads(vah.dump_exact(value))


def parse_response(content_type, body):
    """What a response says: usage, stop_reason, model, whether its stream completed, an API error,
    whether a fallback model served it, and how much text it carried."""
    out = {"usage": None, "stop_reason": None, "model": None, "complete": None, "error": None,
           "served_by_fallback": False, "text_characters": 0}
    try:
        if "text/event-stream" in content_type:
            # A cut-off stream (a cancel, a timed-out turn) can end inside a line, even inside a
            # multi-byte character: that partial line is dropped (an event is whole only at its
            # blank line anyway), so the events before it still price the response.
            if not body.endswith(b"\n"):
                body = body[:body.rfind(b"\n") + 1]
            text = body.decode("utf-8", "replace").encode("utf-8")
            events = list(vah._iter_sse(io.BytesIO(text)))
            for name, data in events:
                if name == "error":
                    out["error"] = json.loads(data).get("error")
            message, complete = vah.accumulate(events)
            out["complete"] = complete
            if message is None:
                return out
            message = plain(message)
        else:
            message = json.loads(body) if body.strip() else None
            if not isinstance(message, dict):
                return out
            if message.get("type") == "error":
                out["error"] = message.get("error")
                return out
        out["usage"] = message.get("usage") if isinstance(message.get("usage"), dict) else None
        out["stop_reason"] = message.get("stop_reason")
        out["model"] = message.get("model")
        blocks = message.get("content") if isinstance(message.get("content"), list) else []
        out["text_characters"] = sum(len(block.get("text") or block.get("thinking") or "") for block in blocks
                                     if isinstance(block, dict))
        iterations = (out["usage"] or {}).get("iterations") or []
        out["served_by_fallback"] = (any(isinstance(block, dict) and block.get("type") == "fallback"
                                         for block in blocks)
                                     or any(isinstance(entry, dict) and entry.get("type") == "fallback_message"
                                            for entry in iterations))
    except (ValueError, KeyError, IndexError, TypeError, AttributeError) as error:
        out["parse_error"] = f"{type(error).__name__}: {error}"
    return out


# ---- The recording proxy.

HOP_BY_HOP = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "proxy-connection", "te",
              "trailer", "trailers", "transfer-encoding", "upgrade"}


def _connection_tokens(values):
    return {token.strip().lower() for value in values or [] for token in value.split(",") if token.strip()}


def _api_error(kind, message):
    return {"type": "error", "error": {"type": kind, "message": message}}


class ProxyServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False
    proxy = None

    def handle_error(self, request, client_address):
        error = sys.exc_info()[1]
        if not isinstance(error, (ConnectionError, TimeoutError)):
            self.proxy.problems.append(self.proxy.redact(f"the proxy failed serving a request: "
                                                         f"{type(error).__name__}: {error}"))


class ProxyHandler(vah.StubHandler):
    """One helper connection: each request forwarded upstream, its response streamed back as it
    arrives (re-chunked when the upstream gave no length), both recorded by `proxy`."""

    proxy = None
    timeout = UPSTREAM_SECONDS

    def do_GET(self):
        self.forward()

    do_POST = do_PUT = do_PATCH = do_DELETE = do_GET

    def send_local(self, status, value):
        data = json.dumps(value).encode()
        self.send_response_only(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        self.wfile.flush()
        return data

    def forward(self):
        proxy = self.proxy
        body = self._body()
        record = proxy.begin(self.command, self.path, self.headers)
        response = bytearray()
        try:
            if proxy.over_budget.is_set() and self.command == "POST":
                record.update(status=400, local="budget", content_type="application/json")
                response += self.send_local(400, _api_error(
                    "invalid_request_error", f"live_assistant_acceptance: the run's ${proxy.budget:.2f} budget is "
                    "spent, so this request was not sent"))
                return
            self.relay(record, body, response)
        except Exception as error:  # noqa: BLE001 - recorded (redacted), the connection closed
            record["proxy_error"] = proxy.redact(f"{type(error).__name__}: {error}")
            self.close_connection = True
        finally:
            proxy.finish(record, body, bytes(response))

    def relay(self, record, body, response):
        proxy = self.proxy
        drop = HOP_BY_HOP | _connection_tokens(self.headers.get_all("Connection")) | {
            "host", "content-length", "accept-encoding"}
        headers = [(name, value) for name, value in self.headers.items() if name.lower() not in drop]
        headers.append(("Accept-Encoding", "identity"))
        if body or self.command in ("POST", "PUT", "PATCH"):
            headers.append(("Content-Length", str(len(body))))
        upstream = proxy.connect()
        try:
            try:
                upstream.putrequest(self.command, proxy.base_path + self.path, skip_accept_encoding=True)
                for name, value in headers:
                    upstream.putheader(name, value)
                upstream.endheaders(body or None)
                reply = upstream.getresponse()
            except (OSError, http.client.HTTPException, ValueError) as error:
                record["upstream_error"] = proxy.redact(f"{type(error).__name__}: {error}")
                record.update(status=502, local="upstream", content_type="application/json")
                response += self.send_local(502, _api_error("api_error", "the acceptance proxy could not reach the "
                                                                         "upstream API"))
                return
            record.update(status=reply.status, request_id=reply.getheader("request-id"),
                          content_type=reply.getheader("content-type") or "")
            length = reply.getheader("content-length")
            skip = HOP_BY_HOP | _connection_tokens(reply.headers.get_all("Connection")) | {"content-length"}
            self.send_response_only(reply.status, reply.reason)
            for name, value in reply.getheaders():
                if name.lower() not in skip:
                    self.send_header(name, value)
            self.send_header(*(("Content-Length", length) if length is not None else ("Transfer-Encoding", "chunked")))
            self.end_headers()
            while True:
                try:
                    data = reply.read1(65536)
                except (OSError, http.client.HTTPException) as error:
                    record["upstream_error"] = proxy.redact(f"{type(error).__name__}: {error}")
                    self.close_connection = True  # the body breaks off for the helper too
                    return
                if not data:
                    break
                response += data
                try:
                    if length is None:
                        self.chunk(data)
                    else:
                        self.wfile.write(data)
                        self.wfile.flush()
                except OSError:
                    record["client_closed"] = True  # the helper hung up: a cancel
                    self.close_connection = True
                    return
            if length is None:
                try:
                    self.chunk(b"")
                except OSError:
                    record["client_closed"] = True
                    self.close_connection = True
        finally:
            upstream.close()


class RecordingProxy:
    """The helper's api_base: forwards to `upstream`, records every exchange into `folder`."""

    def __init__(self, upstream, folder, budget, redact):
        parts = urllib.parse.urlsplit(upstream)
        self.secure = parts.scheme == "https"
        self.host, self.port = parts.hostname, parts.port or (443 if self.secure else 80)
        self.base_path = parts.path.rstrip("/")
        self.tls = ssl.create_default_context() if self.secure else None
        self.folder, self.budget, self.redact = folder, budget, redact
        (folder / "raw").mkdir(parents=True, exist_ok=True)
        self.journal = (folder / "requests.jsonl").open("a", encoding="utf-8")
        self.condition = threading.Condition()
        self.records, self.bodies, self.problems = {}, {}, []
        self.started = self.in_flight = 0
        self.total_cost = 0.0
        self.over_budget = threading.Event()
        self.label = ""
        self.closed = False
        handler = type("Handler", (ProxyHandler,), {"proxy": self})
        self.server = ProxyServer(("127.0.0.1", 0), handler)
        self.server.proxy = self
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True)
        self.thread.start()

    @property
    def base_url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def connect(self):
        if self.secure:
            return http.client.HTTPSConnection(self.host, self.port, timeout=UPSTREAM_SECONDS, context=self.tls)
        return http.client.HTTPConnection(self.host, self.port, timeout=UPSTREAM_SECONDS)

    def begin(self, method, path, headers):
        with self.condition:
            index = self.started
            self.started += 1
            self.in_flight += 1
        joined = {name: ",".join(headers.get_all(name) or []) or None
                  for name in ("anthropic-beta", "anthropic-version")}
        return {"index": index, "scenario": self.label, "method": method, "path": path,
                "route": urllib.parse.urlsplit(path).path, "anthropic_beta": joined["anthropic-beta"],
                "anthropic_version": joined["anthropic-version"], "status": None, "request_id": None,
                "started_at": utc_now().isoformat(timespec="milliseconds"), "t0": time.monotonic()}

    def _write(self, name, data):
        if self.redact.holds(data):
            data = self.redact.scrub(data)
        (self.folder / "raw" / name).write_bytes(data)
        return f"raw/{name}"

    def finish(self, record, request, response):
        record["seconds"] = round(time.monotonic() - record.pop("t0"), 3)
        content_type = record.pop("content_type", None) or ""  # read to parse the body, never recorded
        parsed = parse_response(content_type, response)
        usage = parsed["usage"] or {}
        cost = billed(usage) if usage else 0.0
        if usage and "text/event-stream" in content_type and not parsed["complete"]:
            streamed = parsed["text_characters"] // 3  # a cut-off stream's output, estimated from its text
            cost += max(0, streamed - _count(usage, "output_tokens")) * RATES["output"] / 1e6
            record["cost_estimated"] = True
        stem = f"{record['index']:04d}-{record['method']}"
        extension = "sse" if "text/event-stream" in content_type else "json"
        record.update(
            request_bytes=len(request), response_bytes=len(response),
            request_sha256=hashlib.sha256(request).hexdigest() if request else None,
            request_file=self._write(f"{stem}-request.json", request) if request else None,
            response_file=self._write(f"{stem}-response.{extension}", response) if response else None,
            usage=usage or None, stop_reason=parsed["stop_reason"], model=parsed["model"],
            complete=parsed["complete"], error=parsed["error"], served_by_fallback=parsed["served_by_fallback"],
            cost_usd=round(cost, 6), secret_in_request=self.redact.holds(request) or None)
        if parsed.get("parse_error"):
            record["parse_error"] = parsed["parse_error"]
        with self.condition:
            self.total_cost += cost
            record["running_cost_usd"] = round(self.total_cost, 6)
            if self.total_cost > self.budget:
                self.over_budget.set()
            self.records[record["index"]] = record
            self.bodies[record["index"]] = (request, response)
            line = {key: value for key, value in record.items() if value is not None}
            if not self.journal.closed:  # a stream still open when a stopped run closed the proxy
                self.journal.write(self.redact(json.dumps(line, ensure_ascii=False, sort_keys=True)) + "\n")
                self.journal.flush()
            self.in_flight -= 1
            self.condition.notify_all()

    def wait_idle(self, seconds=IDLE_SECONDS):
        with self.condition:
            return self.condition.wait_for(lambda: self.in_flight == 0, timeout=seconds)

    def between(self, start, end):
        """The finished records with index in [start, end), in index order. Once the proxy is
        closed (the end of the run) it no longer waits for a stream that is still open."""
        if not self.closed:
            self.wait_idle()
        with self.condition:
            return [self.records[index] for index in range(start, end) if index in self.records]

    def request_body(self, record):
        return self.bodies[record["index"]][0]

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        with self.condition:
            self.closed = True
            self.journal.close()


def posts_of(records):
    return [record for record in records if record["method"] == "POST" and record["route"] == "/v1/messages"]


def cache_read(record):
    return _count(record.get("usage") or {}, "cache_read_input_tokens")


def statuses(records):
    counts = collections.Counter(str(record.get("status")) for record in records)
    return ", ".join(f"{status}×{count}" for status, count in sorted(counts.items())) or "none"


# ---- The owned Doctored.

def _error_result(text):
    return json.dumps({"content": [{"type": "text", "text": text}], "isError": True}, separators=(",", ":"))


class OwnedApp:
    """An owned instance (the root CLAUDE.md recipe) that runs the helper's tool calls."""

    def __init__(self, bundle, log_path):
        self.executable = bundle / app_under_test.EXECUTABLE_IN_BUNDLE
        app_under_test.require_fresh_app(self.executable)
        self.process = self.log = self.socket = None
        self.closed = False
        self.serial = 1000
        self.private = Path(tempfile.mkdtemp(prefix="dr-live-acceptance-app-"))
        try:
            self.socket = mcp_client.scratch_socket()
            environment = dict(os.environ, **verification_isolation.owned_environment(self.private, self.socket))
            for name in ("DR_ASSISTANT_HELPER", "DR_ASSISTANT_BASE_URL"):
                environment.pop(name, None)
            self.log = log_path.open("wb")
            self.process = subprocess.Popen([str(self.executable), "-ApplePersistenceIgnoreState", "YES"],
                                            env=environment, stdout=self.log, stderr=self.log)
            self.client = mcp_client.Client(self.socket, timeout=TOOL_SECONDS)
            mcp_client.wait_ready(self.client, self.process, LAUNCH_SECONDS, str(log_path))
        except BaseException:
            # An instance that started but never answered (or a stop signal while it launched) is
            # ended here: the caller never gets an instance to close.
            self.close()
            raise

    def tool(self, name, **arguments):
        """A tools/call's text content, parsed; raises on isError."""
        result = self.client.rpc("tools/call", {"name": name, "arguments": arguments})
        text = next((item["text"] for item in result.get("content", []) if item.get("type") == "text"), "")
        if result.get("isError"):
            raise ScenarioError(f"the instance refused {name}: {text[:300]}")
        return json.loads(text)

    def tool_names(self):
        return [tool["name"] for tool in self.client.rpc("tools/list", {}).get("tools", [])]

    def run_raw(self, name, arguments_json):
        """runTool: the CallToolResult JSON for one tool_request, and whether it is an error."""
        try:
            arguments = json.loads(arguments_json)
        except ValueError:
            arguments = {}
        if not isinstance(arguments, dict):
            arguments = {}  # as `execute` reads arguments that are not an object
        self.serial += 1
        try:
            reply = self.client.post({"jsonrpc": "2.0", "id": self.serial, "method": "tools/call",
                                      "params": {"name": name, "arguments": arguments}}, timeout=TOOL_SECONDS)
        except (OSError, RuntimeError, ValueError) as error:
            return _error_result(f"The test instance did not answer {name}: {type(error).__name__}: {error}"), True
        if not isinstance(reply, dict) or "result" not in reply:
            message = ((reply or {}).get("error") or {}).get("message") if isinstance(reply, dict) else None
            return _error_result(f"The test instance answered {name} with an error: {message}"), True
        result = reply["result"]
        return json.dumps(result, ensure_ascii=False, separators=(",", ":")), bool(result.get("isError"))

    def document(self, document_id):
        """The facts `systemPrompt()` reads: id, title (displayName), width, height, layer_count."""
        entries = self.tool("list_documents")["documents"]
        entry = next(entry for entry in entries if entry["id"] == document_id)
        return {key: entry[key] for key in ("id", "title", "width", "height", "layer_count")}

    def layer_names(self, document_id):
        return [layer.get("name") for layer in self.tool("get_document", document_id=document_id).get("layers", [])]

    def close(self):
        if self.closed:
            return
        self.closed = True
        try:
            if self.process is not None and self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=10)
        finally:
            if self.log is not None:
                self.log.close()
            mcp_client.remove_scratch(self.socket)
            shutil.rmtree(self.private, ignore_errors=True)


# ---- The helper and the app's side of its session.

class AppHelper(vah.Helper):
    """verify_agent_helper's Helper writing each message as AssistantHelperSession.write does."""

    def write(self, message):
        self.write_line(app_json(message))


class Turn:
    def __init__(self, number, text, start):
        self.number, self.text, self.start, self.end = number, text, start, start
        self.events, self.tool_runs = [], []
        self.cancelled = self.budget_stopped = self.timed_out = False
        self.cancel_index = self.cancel_deltas = None
        self.text_deltas = 0

    @property
    def errors(self):
        return [event.get("message") for event in self.events if event["type"] == "error"]

    @property
    def finished(self):
        return bool(self.events) and self.events[-1]["type"] == "turn_finished"

    @property
    def reply(self):
        return vah.texts(self.events, "text_delta")

    def kinds(self):
        return [event["type"] for event in self.events]

    def report(self):
        return {"turn": self.number, "sent": self.text[:300], "reply": self.reply[:600],
                "events": vah.brief(self.events),
                "tool_runs": self.tool_runs, "cancelled": self.cancelled, "errors": self.errors,
                "requests": [self.start, self.end]}


class Conversation:
    """One Assistant panel session: a helper process configured as the panel configures it."""

    def __init__(self, run, label, configure_line):
        self.run, self.label, self.configure_line = run, label, configure_line
        run.proxy.label = label
        self.start = self.end = run.proxy.started
        self.helper = AppHelper(run.helper_path, run.scratch / label, label,
                                stderr_path=run.folder / f"helper-{label}.log")
        self.app = vah.App(self.helper)
        self.turns = []
        self.ended = False
        self.helper.write_line(configure_line)

    def cancel(self, turn):
        if not turn.cancelled:
            self.app.cancel()
            turn.cancelled = True
            turn.cancel_index = self.run.proxy.started
            turn.cancel_deltas = turn.text_deltas

    def run_tool(self, turn, request):
        name = request.get("name")
        if self.app.cancel_requested:
            result = vah.CANCELLED_RESULT
            turn.tool_runs.append({"name": name, "ran": False})
        else:
            began = time.monotonic()
            result, is_error = self.run.instance.run_raw(name, request.get("arguments_json") or "{}")
            turn.tool_runs.append({"name": name, "ran": True, "is_error": is_error,
                                   "seconds": round(time.monotonic() - began, 2)})
        self.app.answer(request, result)

    def turn(self, text, cancel_after_text=None, cancel_at_tool=False):
        """Sends `text` and plays the turn through turn_finished, as the app would."""
        proxy, seconds = self.run.proxy, self.run.turn_seconds
        if NONCE and not self.turns:
            text = f"{NONCE} {text}"
        turn = Turn(len(self.turns) + 1, text, proxy.started)
        self.turns.append(turn)
        self.app.send(text)
        deadline = time.monotonic() + seconds
        while True:
            if proxy.over_budget.is_set() and self.app.busy and not turn.cancelled:
                turn.budget_stopped = True
                self.cancel(turn)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                if turn.timed_out or not self.app.busy:
                    raise ScenarioError(f"turn {turn.number}: no turn_finished within {seconds:g} s, nor "
                                        f"{CANCEL_DRAIN_SECONDS:g} s after a cancel ({vah.brief(turn.events)})")
                turn.timed_out = True
                self.cancel(turn)
                deadline = time.monotonic() + CANCEL_DRAIN_SECONDS
                continue
            try:
                event = self.app.next(min(0.5, remaining))
            except vah.Silent:
                continue
            turn.events.append(event)
            kind = event["type"]
            if kind == "text_delta":
                turn.text_deltas += 1
                if cancel_after_text and turn.text_deltas >= cancel_after_text:
                    self.cancel(turn)
            elif kind == "tool_request":
                # The response that asked for it is priced before the tool runs, so a reply that
                # passes the budget never gets its tools run (or a next request).
                proxy.wait_idle(5.0)
                if proxy.over_budget.is_set():
                    turn.budget_stopped = True
                if cancel_at_tool or turn.budget_stopped:
                    self.cancel(turn)
                self.run_tool(turn, event)
            elif kind == "turn_finished":
                break
        proxy.wait_idle()
        turn.end = proxy.started
        if turn.timed_out:
            raise ScenarioError(f"turn {turn.number}: no turn_finished within {seconds:g} s; it was cancelled")
        if proxy.over_budget.is_set():
            raise BudgetStop(f"the cost estimate ${proxy.total_cost:.4f} passed the ${proxy.budget:.4g} budget in "
                             f"turn {turn.number}")
        return turn

    def finish(self):
        if self.ended:
            return
        self.ended = True
        try:
            self.helper.end()
        finally:
            self.run.proxy.wait_idle()
            self.end = self.run.proxy.started

    def records(self):
        return self.run.proxy.between(self.start, self.end)


# ---- Scenarios.

class Scenario:
    def __init__(self, name):
        self.name, self.title = name, TITLES.get(name, name)
        self.checks, self.notes, self.conversations = [], [], []
        self.verdict, self.failure, self.headline = None, None, ""
        self.extra, self.reported = {}, False

    def check(self, ok, text):
        self.checks.append((bool(ok), text))
        return bool(ok)

    def records(self):
        return [record for conversation in self.conversations for record in conversation.records()]

    def figures(self):
        records = self.records()
        posts = posts_of(records)
        usages = [record.get("usage") or {} for record in posts]
        return {"requests": len(records), "messages_requests": len(posts), "statuses": statuses(records),
                "input_tokens": sum(_count(u, "input_tokens") for u in usages),
                "output_tokens": sum(_count(u, "output_tokens") for u in usages),
                "cache_read_input_tokens": sum(_count(u, "cache_read_input_tokens") for u in usages),
                "cache_creation_input_tokens": sum(_count(u, "cache_creation_input_tokens") for u in usages),
                "error_events": sum(len(turn.errors) for conversation in self.conversations
                                    for turn in conversation.turns),
                "cost_usd": round(sum(record.get("cost_usd") or 0.0 for record in records), 6),
                "fallback_responses": sum(bool(record.get("served_by_fallback")) for record in posts)}


def common_checks(sc, run, conversation):
    """What every conversation must show: 200s, no error event, finished turns, the app protocol."""
    records = conversation.records()
    turns = conversation.turns
    sc.check(records and all(record.get("status") == 200 for record in records),
             f"every request 200 ({statuses(records)} over {len(records)} requests)")
    errors = [message for turn in turns for message in turn.errors]
    sc.check(not errors, f"no error event ({len(errors)}" + (f": {errors[0]!r})" if errors else ")"))
    sc.check(turns and all(turn.finished for turn in turns), f"turn_finished ended each of the {len(turns)} turns")
    problems = []
    for turn in turns:
        try:
            vah.check_turn(turn.events, errors=turn.errors)
        except AssertionError as error:
            problems.append(f"turn {turn.number}: {error}")
    if conversation.helper.violations:
        problems.append(f"{len(conversation.helper.violations)} stdout lines that are not protocol events")
    sc.check(not problems, "the protocol as the panel reads it" + (f": {problems[0]}" if problems else ""))
    clearing = json.loads(conversation.configure_line)["settings"].get("clear_tool_results") is True
    expected = vah.BASE_BETAS | ({vah.CLEARING_BETA} if clearing else set())
    wrong = [record["index"] for record in posts_of(records)
             if record.get("anthropic_version") != vah.API_VERSION
             or {beta.strip() for beta in (record.get("anthropic_beta") or "").split(",") if beta.strip()} != expected]
    sc.check(not wrong, f"anthropic-version {vah.API_VERSION} and the betas {sorted(expected)} on every Messages "
                        f"request" + (f" (not on {wrong})" if wrong else ""))
    unprefixed = [line for line in conversation.helper.stderr_lines() if not line.startswith(vah.STDERR_PREFIX)]
    if unprefixed:
        sc.notes.append(f"{len(unprefixed)} helper stderr lines without {vah.STDERR_PREFIX!r}")
    for turn in turns:
        sc.notes.append(f"turn {turn.number}: {turn.reply.strip()[:140]!r}")
    return records


def turn_posts(run, turn):
    return posts_of(run.proxy.between(turn.start, turn.end))


def tools_ran(turn):
    return [run["name"] for run in turn.tool_runs if run.get("ran")]


def scenario_s1(run, sc):
    document = run.document_id
    before = run.instance.layer_names(document)
    conversation = run.conversation(sc, "S1")
    turns = [conversation.turn(S1_TURNS[0])]
    first_posts = turn_posts(run, turns[0])
    if first_posts:  # S2's baseline, kept even when a later S1 turn fails
        run.s1 = {"first": first_posts[0], "line": conversation.configure_line}
    turns.append(conversation.turn(S1_TURNS[1]))
    added = run.instance.layer_names(document)
    turns.append(conversation.turn(S1_TURNS[2]))
    undone = run.instance.layer_names(document)
    conversation.finish()
    records = common_checks(sc, run, conversation)
    ran = [tools_ran(turn) for turn in turns]
    sc.check(ran[1] and ran[2], f"tool calls ran on the instance: {[len(names) for names in ran]} per turn "
                                f"({', '.join(name for names in ran for name in names) or 'none'})")
    sc.check(len(added) == len(before) + 1 and "Notes" in added and "Notes" not in before,
             f"after turn 2 the layers are {added} (were {before})")
    sc.check(undone == before, f"after turn 3 the layers are {undone}")
    posts = posts_of(records)
    reads = [cache_read(record) for record in posts]
    sc.check(len(posts) >= 3 and all(value > 0 for value in reads[1:]),
             f"cache_read_input_tokens > 0 on every request after the first: {reads}")
    firsts = [cache_read(turn_posts(run, turn)[0]) if turn_posts(run, turn) else 0 for turn in turns]
    sc.check(firsts[0] < firsts[1] < firsts[2], f"each turn's first request reads more cache: {firsts}")
    sc.headline = (f"{len(posts)} Messages requests, cache read {' → '.join(map(str, firsts))} at each turn's first "
                   "request, "
                   f"{sum(map(len, ran))} tool calls ran, layers {len(before)} → {len(added)} → {len(undone)}")


def scenario_s2(run, sc):
    if run.s1 is None:
        raise ScenarioError("S1 recorded no first request in this invocation (its first turn did not finish), "
                            "so there is nothing to compare")
    conversation = run.conversation(sc, "S2", line=run.s1["line"])
    conversation.turn(S1_TURNS[0])
    conversation.finish()
    records = common_checks(sc, run, conversation)
    posts = posts_of(records)
    if not posts:
        raise ScenarioError("the relaunched helper sent no Messages request")
    first, earlier = posts[0], run.s1["first"]
    sc.check(cache_read(first) > 0, f"the relaunched process's first request read {cache_read(first)} cached tokens")
    mine = vah.raw_members(run.proxy.request_body(first).decode("utf-8"))
    theirs = vah.raw_members(run.proxy.request_body(earlier).decode("utf-8"))
    for field in ("tools", "system"):
        same = mine.get(field) == theirs.get(field)
        sc.check(same, f"its {field} bytes equal S1's first request's ({len(mine.get(field) or '')} bytes)" if same
                 else f"its {field} differs from S1's: "
                      f"{vah.first_difference(mine.get(field) or '', theirs.get(field) or '')}")
    whole = run.proxy.request_body(first) == run.proxy.request_body(earlier)
    sc.notes.append("the whole first request is " +
                    ("byte-identical to S1's" if whole else "not byte-identical to S1's"))
    sc.headline = f"first request read {cache_read(first)} cached tokens; tools and system bytes as S1's"


def _absent(partial, body):
    """Whether the cancelled reply's text is absent from a request body, raw or JSON-escaped."""
    if len(partial.strip()) < 20:
        return True
    escaped = json.dumps(partial, ensure_ascii=False)[1:-1].encode()
    return partial.encode() not in body and escaped not in body


def _no_request_after_cancel(run, turn):
    later = [record["index"] for record in turn_posts(run, turn)
             if turn.cancel_index is not None and record["index"] >= turn.cancel_index]
    return not later, later


def scenario_s3(run, sc):
    conversation = run.conversation(sc, "S3")
    first = conversation.turn(ESSAY, cancel_after_text=CANCEL_AFTER_TEXT_DELTAS)
    second = conversation.turn(JUST_OK)
    conversation.finish()
    common_checks(sc, run, conversation)
    sc.check(first.cancelled, f"cancel sent at text delta {first.cancel_deltas} ({first.text_deltas} had arrived by "
                              "turn_finished)" if first.cancelled else
             f"the reply finished after {first.text_deltas} text deltas, before the {CANCEL_AFTER_TEXT_DELTAS}th")
    streamed = turn_posts(run, first)
    cut = streamed[-1] if streamed else {}
    sc.check(cut and not cut.get("complete"),
             f"the cancelled stream was cut off at the proxy after {cut.get('response_bytes')} bytes "
             f"({'the helper hung up' if cut.get('client_closed') else 'no hang-up seen'})" if cut else
             "no streamed request to cancel")
    quiet, later = _no_request_after_cancel(run, first)
    sc.check(quiet, "no request followed the cancel" + (f" (requests {later})" if later else ""))
    after = turn_posts(run, second)
    reply = second.reply.strip()
    sc.check(after and all(record.get("status") == 200 for record in after) and reply,
             f"'{JUST_OK}' got {statuses(after)} and the reply {reply[:40]!r}")
    partial = first.reply
    sc.check(after and _absent(partial, run.proxy.request_body(after[0])),
             f"the next request holds none of the {len(partial)} cancelled characters")
    sc.headline = (f"cancel at text delta {first.cancel_deltas}, stream cut at {cut.get('response_bytes')} bytes, "
                   f"then {statuses(after)} and {reply[:20]!r}")


def scenario_s4(run, sc):
    conversation = run.conversation(sc, "S4")
    first = conversation.turn(RENDER_ASK, cancel_at_tool=True)
    second = conversation.turn(JUST_OK)
    conversation.finish()
    common_checks(sc, run, conversation)
    requested = [event.get("name") for event in first.events if event["type"] == "tool_request"]
    sc.check(first.cancelled and requested, f"cancel sent at the first tool_request ({', '.join(requested) or 'none'})")
    shown = [(event.get("is_error"), event.get("summary")) for event in first.events if event["type"] == "tool_result"]
    sc.check(shown and all(is_error is True and summary == "Cancelled by the user." for is_error, summary in shown),
             f"every pending call answered 'Cancelled by the user.' ({len(shown)})")
    sc.check(not tools_ran(first), f"no tool ran after the cancel ({tools_ran(first) or 'none'})")
    quiet, later = _no_request_after_cancel(run, first)
    sc.check(quiet, "no request followed the cancel" + (f" (requests {later})" if later else ""))
    after = turn_posts(run, second)
    reply = second.reply.strip()
    sc.check(after and all(record.get("status") == 200 for record in after) and reply,
             f"'{JUST_OK}' got {statuses(after)} (no 400 about an unanswered tool_use) and {reply[:40]!r}")
    sc.headline = f"cancel at {requested[0] if requested else 'no'} tool_request, then {statuses(after)}"


def scenario_s5(run, sc):
    before = run.instance.layer_names(run.document_id)
    conversation = run.conversation(sc, "S5")
    turns = [conversation.turn(text) for text in S5_TURNS]
    conversation.finish()
    records = common_checks(sc, run, conversation)
    refused = [record for record in records if record.get("status") != 200 and re.search(
        r"thinking|prefix|tool_use", json.dumps(record.get("error") or {}), re.I)]
    sc.check(not refused, f"no 400 mentioning thinking, prefix or tool_use ({len(refused)})")
    ran = [len(tools_ran(turn)) for turn in turns]
    sc.check(sum(ran[1:6]) >= 4, f"tool calls ran on the instance: {ran} per turn")
    posts = posts_of(records)
    usages = [record.get("usage") or {} for record in posts]
    read = sum(_count(u, "cache_read_input_tokens") for u in usages)
    prompt = sum(_count(u, k) for u in usages for k in ("input_tokens", "cache_read_input_tokens",
                                                        "cache_creation_input_tokens"))
    ratio = read / prompt if prompt else 0.0
    after = run.instance.layer_names(run.document_id)
    sc.notes.append(f"the document's layers went from {before} to {after}")
    sc.headline = f"{len(posts)} Messages requests over 8 turns, cache hit ratio {ratio:.1%} ({read}/{prompt} tokens)"
    sc.extra = {"cache_hit_ratio": round(ratio, 4)}


LAYER_WORDS = ("Sky", "Harbour", "Boats", "Water", "Lamps", "Title", "Vignette", "Grain")
ACTION_WORDS = ("raised the curves midpoint by", "lowered exposure by", "added saturation of", "shifted the hue by",
                "feathered the mask by", "sharpened with an amount of", "blurred with a radius of",
                "lifted the shadows by")
REGION_WORDS = ("upper third", "left edge", "highlights", "midtones", "shadows", "centre", "reflections")
CHECK_WORDS = ("the histogram", "the clipping warning", "a 100% crop", "the soft proof", "a before/after split")
RESULT_WORDS = ("kept it", "reverted it", "kept half of it", "masked it to the left side", "moved it to a new layer")


def editing_notes(seed):
    rng = random.Random(seed)
    lines, size = [], 0
    while size < S6_NOTES_CHARACTERS:
        line = (f"{len(lines) + 1:04d}. {rng.choice(LAYER_WORDS)}: {rng.choice(ACTION_WORDS)} {rng.randint(1, 40)} "
                f"units in the {rng.choice(REGION_WORDS)}; checked {rng.choice(CHECK_WORDS)}; "
                f"{rng.choice(RESULT_WORDS)}.")
        lines.append(line)
        size += len(line) + 1
    return ("Here are my editing notes from an earlier session. Read them, then tell me in one sentence which layer "
            "I worked on most. Do not call any tools.\n\n" + "\n".join(lines))


def scenario_s6(run, sc):
    floor = run.prompt.compact_floor
    conversation = run.conversation(sc, "S6", settings=run.prompt.settings(auto_compact_tokens=floor))
    compacting = None
    for round_number in range(S6_ROUNDS):
        turn = conversation.turn(editing_notes(2500 + round_number))
        if "compaction_started" in turn.kinds():
            compacting = turn
            break
    after = conversation.turn(JUST_OK)
    conversation.finish()
    common_checks(sc, run, conversation)
    used = [event.get("used") for turn in conversation.turns for event in turn.events
            if event["type"] == "context_usage"]
    if compacting is None:
        sc.check(False, f"no compaction_started after {S6_ROUNDS} bulk turns (context_usage {used}, limit {floor})")
        return
    kinds = compacting.kinds()
    failed = [event.get("reason") for turn in conversation.turns for event in turn.events
              if event["type"] == "compaction_failed"]
    started = next(event for event in compacting.events if event["type"] == "compaction_started")
    done = next((event for event in compacting.events if event["type"] == "compacted"), None)
    sc.check(done is not None and kinds.index("compaction_started") < kinds.index("compacted") and not failed,
             f"compaction_started (used {started.get('used')} ≥ {floor}) then compacted "
             f"({done.get('before') if done else '?'} → {done.get('after') if done else '?'})" +
             (f"; compaction_failed {failed}" if failed else ""))
    summaries = [record for record in turn_posts(run, compacting)
                 if "compaction" in vah.raw_members(run.proxy.request_body(record).decode("utf-8"))]
    sc.check(summaries and summaries[-1].get("status") == 200 and summaries[-1].get("stop_reason") == "compaction",
             "the summary request: " + (f"{summaries[-1].get('status')}, stop_reason {summaries[-1].get('stop_reason')}"
                                        if summaries else "none was sent"))
    following = turn_posts(run, after)
    led = False
    if following:
        messages = json.loads(run.proxy.request_body(following[0])).get("messages") or [{}]
        content = messages[0].get("content") if isinstance(messages[0], dict) else None
        led = isinstance(content, list) and bool(content) and content[0].get("type") == "compaction"
    reply = after.reply.strip()
    sc.check(following and all(record.get("status") == 200 for record in following) and led and reply,
             f"the next turn's request, led by the summary block ({led}), got {statuses(following)} and {reply[:30]!r}")
    sc.headline = (f"compacted at the end of bulk turn {compacting.number} "
                   f"({done.get('before') if done else '?'} → {done.get('after') if done else '?'} tokens), then "
                   f"{statuses(following)}")


SCENARIO_BODIES = {"S1": scenario_s1, "S2": scenario_s2, "S3": scenario_s3, "S4": scenario_s4, "S5": scenario_s5,
                   "S6": scenario_s6}


# ---- Self-test: the stub's script for each scenario.

STUB_SIGNATURE = "EqQBCkYIBxgCKkCacceptance0signature+stub/DDDD=="


class ScriptedReply(vah.Reply):
    """vah.Reply plus the compaction summary's block and a message_delta carrying iterations."""

    def compaction(self, summary):
        self._start({"type": "compaction", "content": None})
        self._delta({"type": "compaction_delta", "content": summary})
        self._stop()
        return self

    def end_with(self, stop_reason, usage):
        self._event("message_delta", {"type": "message_delta",
                                      "delta": {"stop_reason": stop_reason, "stop_sequence": None}, "usage": usage})
        self._event("message_stop", {"type": "message_stop"})
        return vah.Stream(self.events)


class StubScript:
    """A conversation's scripted replies; request k reads 3000 + 400k cached tokens (the first reads
    `first_read`), so the cache checks see reads that grow as the live API's do."""

    def __init__(self, label, first_read=0):
        self.label, self.first_read, self.step, self.replies = label, first_read, 0, []

    def _reply(self):
        read = self.first_read if self.step == 0 else 3000 + 400 * self.step
        reply = ScriptedReply(f"msg_{self.label}_{self.step}", vah.usage(250, cache_read=read, cache_creation=400))
        self.step += 1
        return reply

    def tool(self, name, arguments):
        self.replies.append(self._reply().thinking(["Checking the document."], STUB_SIGNATURE)
                            .tool_use(f"toolu_{self.label}_{self.step}", name, [json.dumps(arguments)])
                            .end("tool_use", 40))
        return self

    def text(self, *chunks, think=False):
        reply = self._reply()
        if think:
            reply.thinking([], STUB_SIGNATURE)
        self.replies.append(reply.text(list(chunks)).end("end_turn", 20))
        return self

    def stall(self, count):
        self.replies.append(self._reply().text([f"word{i} " for i in range(count)], close=False).stall())
        return self


def stub_replies(run, label):
    doc = run.document_id
    if label == "S1":
        return (StubScript("s1").tool("get_document", {"document_id": doc}).text("It is a 400×400 ", "RGB image.")
                .tool("new_layer", {"document_id": doc, "name": "Notes"}).text("Added Notes; 2 layers now.")
                .tool("undo", {"document_id": doc}).text("Undone; 1 layer again.")).replies
    if label == "S2":
        return StubScript("s2", first_read=3000).text("The same 400×400 image.").replies
    if label == "S3":
        return StubScript("s3").stall(CANCEL_AFTER_TEXT_DELTAS * 2).text("OK").replies
    if label == "S4":
        return StubScript("s4").tool("render", {"document_id": doc}).text("OK").replies
    if label == "S5":
        index = len(run.instance.layer_names(doc))  # the new layer lands above the (top) active layer
        return (StubScript("s5").tool("get_document", {"document_id": doc}).text("One layer, fully opaque.")
                .tool("new_layer", {"document_id": doc, "name": "Draft"}).text("Added Draft.")
                .tool("set_layer_properties", {"document_id": doc, "index": index, "name": "Sketch"}).text("Renamed.")
                .tool("set_layer_properties", {"document_id": doc, "index": index, "opacity": 0.4}).text("At 40%.")
                .tool("undo", {"document_id": doc}).text("Back to 100%.")
                .tool("render", {"document_id": doc}).text("Shapes on transparency.")
                .text("None of them: the new layer is empty.", think=True).text("- Added Sketch.")).replies
    if label == "S6":
        big = ScriptedReply("msg_s6_0", vah.usage(1500, cache_read=0, cache_creation=23_000)).text(["Title."])
        summary = (ScriptedReply("msg_s6_1", vah.usage(24_600, cache_read=0, cache_creation=0))
                   .compaction("The user shared editing notes; the Title layer was worked on most.")
                   .end_with("compaction", {"output_tokens": 180, "iterations": [
                       {"type": "compaction", "input_tokens": 24_600, "output_tokens": 180}]}))
        reply = ScriptedReply("msg_s6_2", vah.usage(300, cache_read=0, cache_creation=200)).text(["OK"])
        return [big.end("end_turn", 12), summary, reply.end("end_turn", 2)]
    raise KeyError(label)


def cut_stream_priced(sc):
    """A stream cut off inside a multi-byte character (a cancel, a timed-out turn) is still priced
    from the usage its message_start carried."""
    events = (ScriptedReply("msg_cut", vah.usage(5000, cache_read=7000))
              .text(["An em dash \u2014 cut here, mid-character"]).end("end_turn", 30).events)
    body = "".join(f"event: {name}\ndata: {data}\n\n" for name, data in events).encode("utf-8")
    cut = body[:body.index("\u2014".encode("utf-8")) + 1]
    parsed = parse_response("text/event-stream; charset=utf-8", cut)
    usage = parsed["usage"] or {}
    cost = billed(usage) if usage else 0.0
    sc.check(_count(usage, "input_tokens") == 5000 and _count(usage, "cache_read_input_tokens") == 7000
             and parsed["complete"] is False and not parsed.get("parse_error") and cost > 0.02,
             f"a stream cut inside a multi-byte character is priced from its message_start: ${cost:.4f} "
             f"(usage {'read' if usage else 'lost'}, {parsed.get('parse_error') or 'no parse error'})")


def self_test_budget_guard(run):
    """A second stub behind a proxy with a $0.001 budget: the first reply's cost passes it, so the
    driver cancels the turn at its tool_request and stops, and the proxy refuses the next POST
    itself without forwarding it."""
    sc = Scenario("budget-guard")
    sc.title = "budget guard"
    stub = vah.StubAPI()
    proxy = RecordingProxy(stub.base_url, run.folder / "budget-guard", 0.001, run.redact)
    guard = copy.copy(run)
    guard.proxy = proxy
    try:
        stub.script(StubScript("guard").tool("render", {"document_id": run.document_id}).replies[0])
        conversation = guard.conversation(sc, "budget-guard")
        try:
            conversation.turn(RENDER_ASK)
            sc.check(False, "the turn ended without a budget stop")
        except BudgetStop as stop:
            sc.check(True, f"the run stopped: {stop}")
        turn = conversation.turns[0]
        sc.check(turn.budget_stopped and turn.cancelled and not tools_ran(turn),
                 "the turn was cancelled at its tool_request and no tool ran")
        conversation.finish()
        connection = http.client.HTTPConnection("127.0.0.1", proxy.server.server_address[1], timeout=10)
        connection.request("POST", "/v1/messages", body=b'{"model":"x"}', headers={"Content-Type": "application/json"})
        response = connection.getresponse()
        body = json.loads(response.read())
        connection.close()
        proxy.wait_idle()
        sc.check(response.status == 400 and "budget" in body["error"]["message"] and len(stub.posts()) == 1,
                 f"a later POST got a local {response.status} and the stub saw {len(stub.posts())} request(s)")
        cut_stream_priced(sc)
        sc.headline = (f"a ${proxy.budget:.4g} budget passed by the first reply (${proxy.total_cost:.4f}): turn "
                       "cancelled, run stopped, the next POST refused locally")
    except Exception as error:  # noqa: BLE001 - reported as the check's failure
        sc.check(False, run.redact(describe(error)))
    finally:
        for conversation in sc.conversations:
            conversation.finish()
        proxy.close()
        stub.close()
    sc.verdict = "PASS" if all(ok for ok, _ in sc.checks) else "FAIL"
    return sc


# ---- The run.

def describe(error):
    return str(error) if isinstance(error, AssertionError) else f"{type(error).__name__}: {error}"


class Run:
    def __init__(self, mode, folder, bundle, helper_path, prompt, tools_text, key, redact, proxy, instance,
                 document_id):
        self.mode, self.folder, self.bundle, self.helper_path = mode, folder, bundle, helper_path
        self.prompt, self.tools_text, self.key, self.redact = prompt, tools_text, key, redact
        self.proxy, self.instance, self.document_id = proxy, instance, document_id
        self.turn_seconds = TURN_SECONDS[mode]
        self.scratch = Path(tempfile.mkdtemp(prefix="dr-live-acceptance-helpers-"))
        self.s1 = None

    def configure_line(self, settings=None):
        """`configure` as AssistantHelperSession.open writes it, for the document as it is now."""
        document = self.instance.document(self.document_id)
        return app_json({"type": "configure", "api_key": self.key, "model": self.prompt.model,
                         "system": self.prompt.system(document), "tools": Raw(self.tools_text),
                         "settings": settings or self.prompt.settings(), "core_tools": self.prompt.core_tools,
                         "search_guide": self.prompt.search_guide,
                         "compaction_instructions": self.prompt.compaction_instructions,
                         "api_base": self.proxy.base_url})

    def conversation(self, sc, label, settings=None, line=None):
        conversation = Conversation(self, label, line or self.configure_line(settings))
        sc.conversations.append(conversation)
        return conversation


def run_scenario(run, sc, stub):
    name = sc.name
    if stub is not None:
        stub.script(*stub_replies(run, name))
    try:
        SCENARIO_BODIES[name](run, sc)
        sc.verdict = "PASS" if all(ok for ok, _ in sc.checks) else "FAIL"
    except KeyboardInterrupt as stop:  # a stop signal: its conversations still end, then the run does
        sc.verdict, sc.failure = "FAIL", f"interrupted by {getattr(stop, 'name', 'SIGINT')}"
        raise
    except BudgetStop as stop:
        sc.verdict, sc.failure = "STOP", run.redact(str(stop))
    except Exception as error:  # noqa: BLE001 - every scenario reports, then the run goes on
        sc.verdict, sc.failure = "FAIL", run.redact(describe(error))
        if not isinstance(error, AssertionError):
            sc.notes.append(run.redact(traceback.format_exc(limit=4).strip().splitlines()[-3]))
        for conversation in sc.conversations:
            tail = conversation.helper.stderr_tail()
            if tail:
                sc.notes.append(run.redact(f"stderr of {conversation.label}: {tail}"))
    finally:
        for conversation in sc.conversations:
            try:
                conversation.finish()
            except Exception as error:  # noqa: BLE001 - cleanup goes on
                sc.notes.append(run.redact(f"ending {conversation.label}: {describe(error)}"))
    if stub is not None:
        with stub.condition:
            left, problems = len(stub.replies), list(stub.problems)
            stub.replies.clear()
            stub.problems.clear()
        if left or problems:
            sc.verdict = "FAIL" if sc.verdict == "PASS" else sc.verdict
            sc.check(False, f"the stub's script: {left} replies unused, problems {problems}")
    return sc


def report(sc):
    sc.reported = True
    figures = sc.figures() if sc.conversations else {}
    head = f"{sc.verdict} {sc.name} {sc.title}"
    detail = sc.failure or sc.headline
    cost = f", ${figures['cost_usd']:.4f}" if figures else ""
    say(f"{head}: {detail}{cost}")
    for ok, text in sc.checks:
        say(f"  {'ok  ' if ok else 'FAIL'} {text}")
    for note in sc.notes:
        say(f"  note: {note}")


def table(results, total_cost, budget):
    columns = ("scenario", "verdict", "requests", "statuses", "cache read", "cache creation", "input", "output",
               "error events", "cost")
    rows = []
    for sc in results:
        f = sc.figures() if sc.conversations else {}
        rows.append((sc.name, sc.verdict or "SKIP", str(f.get("requests", 0)), f.get("statuses", "none"),
                     f"{f.get('cache_read_input_tokens', 0):,}", f"{f.get('cache_creation_input_tokens', 0):,}",
                     f"{f.get('input_tokens', 0):,}", f"{f.get('output_tokens', 0):,}", str(f.get("error_events", 0)),
                     f"${f.get('cost_usd', 0.0):.4f}"))
    rows.append(("total", "", "", "", "", "", "", "", "", f"${total_cost:.4f} of ${budget:.2f}"))
    widths = [max(len(row[i]) for row in [columns, *rows]) for i in range(len(columns))]
    for row in [columns, *rows]:
        say("  ".join(cell.ljust(width) for cell, width in zip(row, widths)).rstrip())


def parse_arguments():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--self-test", action="store_true", help="the loopback stub upstream and a dummy key")
    mode.add_argument("--live", action="store_true", help="https://api.anthropic.com with the key from --key-file")
    parser.add_argument("--yes-spend", action="store_true", help="required with --live: the run spends real money")
    parser.add_argument("--budget-usd", type=float, default=5.00, help="stop once the cost estimate passes this")
    parser.add_argument("--key-file", type=Path, help="live only; default the app's anthropic_api_key file")
    parser.add_argument("--app", type=Path, help="the Doctored.app to launch and take the helper and catalog from "
                                                 "(default the worktree's Release bundle)")
    parser.add_argument("--image", type=Path, default=DEFAULT_IMAGE, help="the image the instance opens")
    parser.add_argument("--nonce", action="store_true",
                        help="mark each conversation's first message with this run's time, so cached prefixes "
                             "an earlier run left are not read")
    parser.add_argument("--scenarios", help="comma-separated, from " + ",".join(SCENARIOS) + " (S2 needs S1)")
    arguments = parser.parse_args()
    if arguments.live and not arguments.yes_spend:
        parser.error("--live spends real money on the Claude API; pass --yes-spend to run it (it never prompts)")
    if arguments.self_test and (arguments.key_file or arguments.yes_spend):
        parser.error("--self-test uses its own dummy key and spends nothing; --key-file and --yes-spend are for --live")
    if not arguments.budget_usd > 0:
        parser.error("--budget-usd must be positive")
    selected = [name.strip().upper() for name in (arguments.scenarios or ",".join(SCENARIOS)).split(",")
                if name.strip()]
    unknown = [name for name in selected if name not in SCENARIOS]
    if unknown:
        parser.error(f"unknown scenarios {unknown}")
    if "S2" in selected and "S1" not in selected:
        parser.error("S2 compares against S1's first request; select S1 too")
    arguments.selected = [name for name in SCENARIOS if name in selected]
    if arguments.nonce:
        global NONCE
        NONCE = f"(Run {time.strftime('%Y%m%dT%H%M%S', time.gmtime())}.)"
    return arguments


# ---- Stopping: a signal ends the run the way a failure does.

STOP_SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)


class Interrupted(KeyboardInterrupt):
    """A stop signal, raised in the main thread so every `finally` runs: the instance and the
    helpers are ended, the run folder is scanned and summary.json is written."""

    def __init__(self, signum):
        super().__init__(signum)
        self.signum = int(signum)
        self.name = signal.Signals(signum).name


def _stop(signum, frame):
    # Later signals are ignored, so the cleanup this one starts is not cut short in its turn. It is
    # bounded (the instance's 25 s, a helper's 12 s), and SIGKILL still ends the process.
    for each in STOP_SIGNALS:
        signal.signal(each, signal.SIG_IGN)
    raise Interrupted(signum)


def install_stop_handlers():
    """SIGINT, SIGTERM and SIGHUP raise `Interrupted`; one ignored at start (nohup) stays ignored."""
    for each in STOP_SIGNALS:
        if signal.getsignal(each) is not signal.SIG_IGN:
            signal.signal(each, _stop)


class Outcome:
    """What the end of the run reports, filled in as the run goes, so a run that fails (an instance
    that never answers) or is stopped (a signal) still writes its summary."""

    def __init__(self, mode, arguments, folder, bundle, helper_path, model, image):
        self.mode, self.folder, self.bundle, self.helper_path = mode, folder, bundle, helper_path
        self.model, self.image = model, image
        self.budget, self.selected = arguments.budget_usd, arguments.selected
        self.started = utc_now()
        self.results, self.guard, self.notes, self.proxy = [], None, [], None
        self.interrupted = self.error = None


def execute(arguments, o, prompt, tools_text, tool_names, json_note):
    """The run itself: the key, the proxy, the owned instance and each scenario, then the cleanup of
    everything it started (each step even when an earlier one fails)."""
    mode, folder = o.mode, o.folder
    key_folder = stub = instance = run = None
    try:
        if mode == "live":
            key = read_key(arguments.key_file or DEFAULT_KEY_FILE)
            upstream = LIVE_UPSTREAM
        else:
            key_folder = Path(tempfile.mkdtemp(prefix="dr-live-acceptance-key-"))
            key_file = key_folder / "anthropic_api_key"
            key_file.write_text(SELF_TEST_KEY + "\n", encoding="utf-8")
            key_file.chmod(0o600)
            key = read_key(key_file)
            stub = vah.StubAPI()
            upstream = stub.base_url
            assert urllib.parse.urlsplit(upstream).hostname == "127.0.0.1", "the self-test must never leave loopback"
        REDACT.secrets = [key]
        say(f"live_assistant_acceptance --{mode}: run folder {folder.relative_to(ROOT)}")
        helper = o.helper_path
        say(f"helper: {helper.relative_to(ROOT) if helper.is_relative_to(ROOT) else helper}\n"
            f"model: {prompt.model}; {len(tool_names)} tools ({Path(vah.CATALOG_IN_BUNDLE).name} minus "
            f"{', '.join(sorted(prompt.excluded))}); {json_note}\n"
            f"upstream: {upstream if mode == 'live' else 'the loopback stub'}; budget ${arguments.budget_usd:.2f}")
        o.proxy = proxy = RecordingProxy(upstream, folder, arguments.budget_usd, REDACT)
        instance = OwnedApp(o.bundle, folder / "app.log")
        served = set(instance.tool_names())
        if served != set(tool_names) | prompt.excluded:
            o.notes.append(f"the instance's tools/list differs from the bundle catalog: "
                           f"{sorted(served ^ (set(tool_names) | prompt.excluded))[:8]}")
        document_id = instance.tool("open_document", path=str(o.image))["document"]["id"]
        document = instance.document(document_id)
        say(f"document {document_id}: {document['title']} {document['width']}×{document['height']}, "
            f"{document['layer_count']} layer(s)\n")
        run = Run(mode, folder, o.bundle, helper, prompt, tools_text, key, REDACT, proxy, instance, document_id)
        for name in arguments.selected:
            sc = Scenario(name)
            o.results.append(sc)
            if proxy.over_budget.is_set():
                sc.verdict, sc.failure = "SKIP", "the budget is spent"
            else:
                run_scenario(run, sc, stub)
            report(sc)
        if mode == "self-test":
            o.guard = self_test_budget_guard(run)
            report(o.guard)
    finally:
        steps = [("the instance", instance and instance.close), ("the proxy", o.proxy and o.proxy.close),
                 ("the stub", stub and stub.close),
                 ("the key folder", key_folder and (lambda: shutil.rmtree(key_folder, ignore_errors=True))),
                 ("the helper folders", run and (lambda: shutil.rmtree(run.scratch, ignore_errors=True)))]
        for what, step in steps:
            if not step:
                continue
            try:
                step()
            except Exception as error:  # noqa: BLE001 - the other steps still run
                o.notes.append(REDACT(f"ending {what}: {describe(error)}"))


def conclude(o):
    """The end of every run, passed, failed or stopped: the secrets scan, summary.json, then the
    table on stdout (written after the file, so a closed stdout loses nothing). Whether it passed."""
    leaked = scan_for_secrets(o.folder, REDACT)
    proxy = o.proxy
    total = proxy.total_cost if proxy is not None else 0.0
    figures = {id(sc): sc.figures() if sc.conversations else {} for sc in o.results}
    fallbacks = sum(f.get("fallback_responses", 0) for f in figures.values())
    notes = list(o.notes)
    if o.interrupted:
        notes.append(f"the run was stopped by {o.interrupted}: {len(o.results)} of {len(o.selected)} scenarios "
                     "began; the instance and helpers were ended")
    if o.error:
        notes.append(f"the run failed: {o.error}")
    notes.append(NOT_ATTEMPTED)
    notes.append(f"{fallbacks} response(s) were served by a fallback model" if fallbacks else
                 "no response was served by a fallback model (fallbacks: \"default\" is sent; a fallback cannot be "
                 "forced)")
    if proxy is not None:
        notes.extend(proxy.problems)
    passed = (o.interrupted is None and o.error is None and len(o.results) == len(o.selected)
              and all(sc.verdict == "PASS" for sc in o.results) and not leaked
              and (o.mode == "live" or (o.guard is not None and o.guard.verdict == "PASS")))
    summary = {
        "mode": o.mode, "started": o.started.isoformat(timespec="seconds"),
        "finished": utc_now().isoformat(timespec="seconds"), "run_folder": str(o.folder.relative_to(ROOT)),
        "app": str(o.bundle), "helper": str(o.helper_path), "model": o.model, "image": str(o.image),
        "budget_usd": o.budget, "total_cost_usd": round(total, 6),
        "verdict": "PASS" if passed else "FAIL", "interrupted": o.interrupted, "error": o.error,
        "secrets_found_in": leaked, "notes": notes,
        "scenarios": [{"name": sc.name, "title": sc.title, "verdict": sc.verdict or "FAIL", "failure": sc.failure,
                       "headline": sc.headline, "checks": [{"ok": ok, "text": text} for ok, text in sc.checks],
                       "notes": sc.notes, **sc.extra,
                       **(figures[id(sc)] if id(sc) in figures else sc.figures() if sc.conversations else {}),
                       "turns": [turn.report() for conversation in sc.conversations for turn in conversation.turns]}
                      for sc in o.results + ([o.guard] if o.guard else [])]}
    (o.folder / "summary.json").write_text(REDACT(json.dumps(summary, ensure_ascii=False, indent=2)) + "\n",
                                           encoding="utf-8")
    for sc in o.results:
        if not sc.reported:
            sc.verdict = sc.verdict or "FAIL"
            report(sc)
    say()
    table(o.results, total, o.budget)
    say(f"\nrunning total: ${total:.4f} estimated at Claude Opus 5.5 rates (budget ${o.budget:.2f})")
    for note in notes:
        say(f"note: {note}")
    if leaked:
        say(f"FAIL secrets: the key was in {leaked}; those files were scrubbed")
    else:
        say("ok   secrets: no file in the run folder holds the key")
    say(f"\nlive_assistant_acceptance --{o.mode}: {'PASS' if passed else 'FAIL'} "
        f"({sum(sc.verdict == 'PASS' for sc in o.results)}/{len(o.selected)} scenarios"
        f"{', stopped by ' + o.interrupted if o.interrupted else ''}); {o.folder.relative_to(ROOT)}/summary.json")
    return passed


def main():
    arguments = parse_arguments()
    mode = "live" if arguments.live else "self-test"
    bundle = (arguments.app or DEFAULT_APP).expanduser().absolute()
    if bundle.suffix != ".app":
        bundle = bundle.resolve().parents[2]  # an executable inside a bundle
    helper_path = bundle / vah.HELPER_IN_BUNDLE
    catalog_path = bundle / vah.CATALOG_IN_BUNDLE
    for path in (helper_path, catalog_path):
        if not path.is_file():
            raise SystemExit(f"{path} is missing: build the bundle (`make app`) or pass --app")
    vah.require_fresh(helper_path)
    image = arguments.image.expanduser().absolute()
    if not image.is_file():
        raise SystemExit(f"{image} is not a file")
    prompt = AppPrompt()
    tools_text, tool_names, json_note = panel_tools(catalog_path, prompt.excluded)
    stamp = utc_now().strftime("%Y%m%dT%H%M%SZ")
    folder = RUNS / (stamp if mode == "live" else f"{stamp}-self-test")
    folder.mkdir(parents=True, exist_ok=False)

    outcome = Outcome(mode, arguments, folder, bundle, helper_path, prompt.model, image)
    passed = False
    try:
        execute(arguments, outcome, prompt, tools_text, tool_names, json_note)
    except KeyboardInterrupt as stop:
        outcome.interrupted = getattr(stop, "name", "SIGINT")
        raise
    except (Exception, SystemExit) as error:
        outcome.error = REDACT(str(error) if isinstance(error, SystemExit) else describe(error))
        raise
    finally:
        try:
            passed = conclude(outcome)
        except Exception as error:  # noqa: BLE001 - the run's own outcome still decides the exit
            _complain(f"live_assistant_acceptance: the summary failed: {describe(error)}")
    raise SystemExit(0 if passed else 1)


def _complain(text):
    try:
        print(REDACT(text), file=sys.stderr, flush=True)
    except OSError:
        pass


if __name__ == "__main__":
    install_stop_handlers()
    try:
        main()
    except KeyboardInterrupt as stop:
        _complain(f"live_assistant_acceptance: stopped by {getattr(stop, 'name', 'SIGINT')}")
        raise SystemExit(128 + getattr(stop, "signum", int(signal.SIGINT))) from None
    except SystemExit:
        raise
    except BaseException as failure:  # noqa: BLE001 - one redacted line, never a raw traceback
        _complain(f"live_assistant_acceptance: {describe(failure)}")
        raise SystemExit(1) from None
