# agent/CLAUDE.md — the assistant helper's rules

Applies to everything under `agent/`, the SwiftPM package that builds `doctored-agent`:
the Assistant panel's agent loop, bundled as `Doctored.app/Contents/Helpers/doctored-agent`
and started once per panel by `AssistantHelperSession` (protocol and decisions in
`designs/assistant-helper.md`). `make agent-test` runs its suite; project.yml's
"Build Agent Helper" phase and `make agent` build it through `scripts/build_agent_helper.sh`
(SwiftPM release on the build/agent-swift scratch path, file by file in batches rather than
whole-module, which peaks near 10 GB) and sign it (`--options runtime`, no entitlements).
`scripts/verify_agent_helper.py` drives a built helper over stdio against a loopback API;
`scripts/live_assistant_acceptance.py --live --yes-spend` runs it against the real API with the
owner's key and spends money, so only when the owner asks (`--self-test` is free).

## Module map

One line per file; each file's doc comments hold its detail.

- `Package.swift` — swift-tools 6.0, macOS 15, Swift 6 mode; `-warnings-as-errors` on these targets under `DR_AGENT_STRICT=1` (`make agent-test`).
- `Sources/doctored-agent/main.swift` — the stdin reader thread and line loop, the one
  stdout writer, Slaude's diagnostics routed to `Log`, exit 0 at stdin EOF.
- `Protocol.swift` — `HostMessage` (app → helper), `HelperEvent` (helper → app), `Configure`, `EventSink`.
- `LineSplitter.swift` — the LF-only line framing.
- `Session.swift` — `AgentSession`, the actor: configure, send, cancel, settings, tool answers, error wording.
- `Turn.swift` — the turn loop: one streamed request per round, the tool round trip, stop reasons.
- `Compaction.swift` — the end-of-turn summary request and its events.
- `Request.swift` — the request bodies and betas, built once per configure.
- `History.swift` — the verbatim, append-only history and the `pushUserText` merge.
- `ContextUsage.swift` — the context-meter arithmetic and cleared tool results.
- `ToolResults.swift` — MCP CallToolResult → tool_result blocks and the panel's summary.
- `ClientFactory.swift` — the Slaude client per configure, behind the `MessagesAPI` seam.
- `Log.swift` — stderr diagnostics.
- `Tests/DoctoredAgentKitTests/` — XCTest; `Support/` holds the scripted transport, SSE builders, loopback server.

## Rules

- **stdout is protocol only**: one `HelperEvent` per line, written only by `StdoutWriter`.
  Diagnostics go to stderr through `Log`, every line starting `doctored-agent: `.
- **The protocol is the app's contract.** The app reads events loosely, so a renamed type
  or field fails silently there; change one only with its app side and the ADR. The
  strings the panel shows (refusals, `API error <status>: …`, `API error: …`) are pinned by tests.
- **History is verbatim and append-only**: `[RawJSON]` exactly as received, replayed byte
  for byte, which Opus 5.5's thinking replay and the prompt cache need. Never route it
  through typed models, `JSONValue` or sorted keys. The only rewrites are the compaction
  swap at turn end and the `pushUserText` merge into an unanswered user message.
- **Every tool_use gets a tool_result** in the next user message, or the API refuses every
  later request: tools are run or answered (a cancel's "Cancelled by the user.", an unknown
  tool, a final reply's unrun call), never left open.
- **A cancel never ends a tool wait.** Every tool_request of a response goes out at once
  and every answer is awaited (after a cancel the app answers "Cancelled by the user."),
  so every tool_use keeps its tool_result.
- **The client comes from `configure` only**: explicit key and base URL, `environment: [:]`,
  `maxRetries` 3. The helper reads no environment variable, file or profile.
- **Slaude is the only dependency**, pinned by exact revision; a bump edits the revision,
  re-resolves with the scratch path above and commits `Package.resolved`.
- **async/await and actors are this package's concurrency model**; the app keeps its own rule (app/CLAUDE.md, Hygiene).
- Every file stays under 400 lines.
- **Tests use no network and no real key**: dummy keys, Slaude's `MockTransport` through
  `ClientFactory`, and the 127.0.0.1 `LoopbackServer`. `swift test --package-path agent
  --scratch-path build/agent-swift` is the suite (`make agent-test`).
