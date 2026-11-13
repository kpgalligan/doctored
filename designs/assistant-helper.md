# The assistant's agent loop runs in a Swift helper on Slaude (ADR)

- **Status:** Accepted — 2026-11-03; it replaces the Bun-compiled TypeScript helper. Shipped in v0.4.4 (2026-11-04) after the live run with a real key (Verification) and the first publish run's compile (Accepted risks) both passed.
- **Source:** the owner's request of 2026-11-03: replace the helper with a Swift one on the owner's Swift Claude API client, Slaude, and remove the Bun/TypeScript/JavaScript integration of the helper (the `public-docs/` Astro site is not part of it). The TypeScript original is `agent/src/main.ts` and `agent/src/protocol.ts` at `c39d5aa8`, deleted by this batch (`git show c39d5aa8:agent/src/main.ts`); the Protocol section below replaces `protocol.ts`.
- **Related:** [agent-socket.md](agent-socket.md) (the MCP server whose dispatcher every helper tool call reaches); [sparkle-updates.md](sparkle-updates.md) (the publish workflow); `agent/CLAUDE.md` (the helper's own rules).

## What ships

- `Contents/Helpers/doctored-agent`: a Swift executable built from `agent/`, a SwiftPM package on Slaude. It runs the assistant's agent loop for one Assistant panel (one document window) and speaks JSON lines over stdio to `app/Sources/AssistantHelperSession.swift`. It owns no tools: every tool call goes back to the app, which runs it through `AgentServer.execute`, the MCP server's dispatcher.
- The app side is unchanged apart from deleted dead code and doc comments.

## Why Slaude (survey of 2026-11-03)

- **No official client exists in Swift, Rust, C or C++.** Anthropic's SDK list is Python, TypeScript, C#, Go, Java, PHP and Ruby, and the `anthropics` GitHub organization holds no such repository. Its one Swift repository, `anthropics/ClaudeForFoundationModels`, is a Foundation Models adapter that needs macOS 27, and its Messages client has `package` access.
- **No third-party client is clearly better.**
  - Swift: the closest, `wyleung/anthropic-sdk-swift` (MIT), is on `2026-10-21` and types tool search, adaptive thinking and effort. But it had 0 stars, one maintainer (not the owner) and was two months old, synced three TypeScript minor versions behind. It also encodes JSON objects through unordered dictionaries, which breaks a byte-stable prompt prefix. The rest of the Swift field sits behind other abstractions (the Vercel AI SDK ports send `2023-06-01`), drops thinking deltas, or is stale.
  - Rust: `aoprisan/rust-claude-sdk` is the only crate on `2026-10-21`, at a month old with 0 stars and no compaction or context-management types; the maintained Rust options send `2023-06-01` or are frameworks.
  - C or C++: nothing supports thinking.
- **Slaude** (`kpgalligan/Slaude`, Apache-2.0, Foundation only) is a source transformation of the official Python SDK, version for version (anthropic 2.3.0, API version `2026-10-21`), with about 3,800 tests. Its owner is Doctored's. For this helper it gained order-preserving `RawJSON`, `beta.messages.stream(body:)` with a raw-JSON message accumulator, lenient stream decoding, typed mid-stream errors, `CancellationError` on a cancelled stream, and the case-collision renames that macOS's case-insensitive APFS needs (revision `4c6c2ca435350741796c3d461720a61e7149b378`).

## Decisions (AH-1..AH-7)

- **AH-1 — The process boundary and the stdio protocol stay.** The helper is a separate executable, and the protocol below is the TypeScript helper's, minus the dead messages (AH-4). The app side does not change except to delete code that only served removed features. Why:
  - Slaude is async/await throughout, and `app/` forbids async/await and Combine (app/CLAUDE.md, "Hygiene").
  - The app builds in Swift 5 mode. `make typecheck` and every probe set compile `app/Sources` with no extra modules.
  - `AgentServer.execute` asserts main and may enter `BackgroundWork`'s modal wait.
  - A helper crash cannot take a document down.
  - The key arrives only over stdin, in a scrubbed environment.
  - Slaude's release compile (about 150 s and 1.8 GB per compiler process in batches, 10.6 GB as one whole module) stays out of the app target.
- **AH-2 — A SwiftPM package in `agent/`.**
  - swift-tools 6.0, `platforms: [.macOS(.v15)]`, Swift 6 language mode.
  - A library target `DoctoredAgentKit` holds the protocol codec, the tool-result conversion, the history, the request bodies, the turn loop and the context arithmetic, behind a messages-client seam that tests drive over Slaude's injectable transport.
  - The executable product `doctored-agent` holds the stdin loop and the one stdout writer. The tests are in `agent/Tests`.
  - Slaude is pinned by exact revision `4c6c2ca435350741796c3d461720a61e7149b378`, and `agent/Package.resolved` is committed.
  - SwiftPM's scratch path is `build/agent-swift`: ignored, and per lane, since each worktree has its own `build/`. Each configuration's Clang `ModuleCache` there records its absolute path, so a `build/` copied from another checkout failed every compile ("PCH was compiled with module cache path …"). `make agent-test` and `scripts/build_agent_helper.sh` (before SwiftPM runs) first call `scripts/agent_module_cache.py`: when the marker `build/agent-swift/module-cache-made-at` does not name this scratch path, every `ModuleCache` under it is removed, for SwiftPM to make again, by the rule `probe_build.remake_if_moved` applies to the probe builds' cache. Removing them from a build made in place recompiled no Slaude file.
  - The helper's own targets build with `-warnings-as-errors` only under `DR_AGENT_STRICT=1`, which `make agent-test` (and so the gate's suite) sets. The shipping build runs SwiftPM in a scrubbed environment without it, so a newer Xcode on the publish runner that adds a diagnostic cannot fail the archive.
  - Why not an Xcode target that links the package: a standalone `make agent-test`; a committed `Package.resolved` (the app's would sit inside the ignored `Doctored.xcodeproj`, as `project.yml`'s Sparkle comment says); and the `mcp-proxy/` model of a helper with its own build and suite.
- **AH-3 — The helper writes its own turn loop on Slaude's client** (transport, retries, typed HTTP errors, SSE stream, Models API), not on `BetaStreamingToolRunner`. History is `[RawJSON]`, kept exactly as received and replayed verbatim. Each assistant message is the stream's `messageParam`, every block and field in the API's key order. Each request body is built as `RawJSON` and sent as given. Why:
  - Opus 5.5 replays earlier thinking only while the prefix is byte-identical, and a stable prefix keeps the cache warm.
  - The tool dispatch must stay the TypeScript runner's: every `tool_use` of a response asked for at once, and a cancel never resuming a pending wait. Slaude's runner runs tools one at a time and rethrows `CancellationError` from a tool, which can leave a `tool_use` without a result and fail the next request with a 400.
  - Compaction success is one explicit rule: `stop_reason == "compaction"` and a `compaction` block whose `content` is a non-empty string. The API reports a summary it failed to write as `content: null`, a no-op when sent back, so swapping the history for it would drop the conversation.
- **AH-4 — Pricing is removed end to end, with the protocol messages nothing uses.**
  - Pricing was dead in production. The app never sent `price_cache` or `price_feed`, so the LiteLLM refresh never ran, and it handles none of `prices`, `cost` and `message_done` (`AssistantPanelViewController.handleEvent`, `AssistantHelperSession.handle`). About 450 lines and their fixtures bought no visible behaviour.
  - Also removed:
    - `reset`, which the app never sends: Clear closes the process, and a reopen is a new one.
    - The app's `assistant_text` case, which the TypeScript helper never emitted.
  - `ready` stays as the configure acknowledgement, though the app ignores it.
  - What went is listed under "Dropped".
- **AH-5 — API version `2026-10-21`, Slaude's (`ANTHROPIC_API_VERSION`).** The TypeScript SDK sent `2023-06-01`. Checked against the version's changes:
  - (1) The helper sends no `temperature`, `top_p` or `top_k`.
  - (2) Assistant messages are only the API's own replies, replayed verbatim, so no `document` block is placed directly in one. A document nested in a server tool result is still accepted, and the helper offers no tool that returns one.
  - (3) User messages carry the typed text and `tool_result` blocks of text and base64 images built from MCP results, never `citations`.
  - (4) No Admin or Analytics API is called. `GET /v1/models/{id}` behaves exactly as on `2023-06-01`.
  - (5) The official SDKs' own breaking changes do not apply to a raw-JSON body.
  - Slaude sends the key as `Authorization: Bearer`, where the TypeScript SDK sent `X-Api-Key`. This matters only to a proxy behind `DR_ASSISTANT_BASE_URL`, which only test servers use.
- **AH-6 — Signed like `doctored-mcp`: the hardened runtime, no entitlements.**
  - The identity is Xcode's `EXPANDED_CODE_SIGN_IDENTITY`, else ad hoc, with `--timestamp` for a real identity.
  - The helper is signed before Write Agent Catalog seals the bundle.
  - `com.apple.security.cs.allow-jit` was for Bun's JavaScriptCore alone, so `agent/entitlements.plist` is deleted. A leftover entitlement would be a needless hardening regression, and the gate fails on one.
- **AH-7 — async/await is allowed in `agent/` only** (`agent/CLAUDE.md`). `app/Sources` keeps its rule, because nothing of the helper enters the app process.

## Protocol

Newline-delimited JSON, one object per line in each direction. The type name `tool_result` is used both ways, with different shapes. The app reads events loosely (`[String: Any]`) and ignores unknown types and fields. A renamed event or field therefore fails silently: the names and shapes below are the contract.

### Framing and process

- **Helper.**
  - stdout carries the protocol only. Diagnostics go to stderr, each line prefixed `doctored-agent: `.
  - Blank lines are skipped. A line that is not a JSON object, or lacks a field its type needs, is answered `{"type":"error","message":"Bad message from the app: <why>"}`.
  - A line of an unknown type (`reset` among them) is ignored.
  - The helper exits 0 when stdin closes.
- **App, reading** (`AssistantHelperSession`):
  - The pipe's readability handler splits on `\n` and silently drops any line that is not a JSON object.
  - Each object is handled on main.
- **App, writing.** Writes go through a serial queue off main, because a render can be megabytes. The write end has `F_SETNOSIGPIPE`. The helper's stderr is not redirected, so it reaches the app's.
- **Launch.**
  - The app starts the helper lazily on the first Send, one process per Assistant panel.
  - The executable is `DR_ASSISTANT_HELPER` if set, else `Contents/Helpers/doctored-agent`. With neither, the app shows "The assistant helper is not in this build."
  - The environment is exactly `HOME`, `TMPDIR` and `PATH=/usr/bin:/bin`, and there are no arguments. `configure` is written straight after launch.
  - The helper reads no environment variable. Its Slaude client gets the key and base URL from `configure` and an empty `environment`, so `ANTHROPIC_*`, credential profiles and OpenTelemetry settings never apply.
- **Shutdown.**
  - The app's `close()` (on Clear, on a key change, or when the panel goes away) clears its event handler and closes stdin. It sends SIGTERM if the process is still running 2 s later.
  - If the helper exits on its own, the app shows "The assistant helper stopped. Clear the conversation to start a new one." It emits a local `turn_finished` if it was busy and marks the session closed.
  - History lives only in the helper's memory, so a reopen starts an empty conversation.

### App → helper

| Message | Fields | Sent when |
| :--- | :--- | :--- |
| `configure` | `api_key`; `model`; `system`; `tools`; `core_tools`; `search_guide`; `compaction_instructions`; `settings`; `api_base`? | once, straight after launch |
| `send` | `text` | the user sends while the session is not busy (the app then counts itself busy) |
| `cancel` | — | Stop, only while busy (the app also sets its own cancel flag) |
| `settings` | `settings` | a Chat Settings save; applies from the next turn |
| `tool_result` | `call_id` (string); `result`: CallToolResult or `null` | the answer to each `tool_request` |

- **`configure`'s fields:**
  - `api_key` is `APIKeyStore.resolve()`.
  - `model` is user default `AssistantModel`, else `claude-opus-5-5`.
  - `system` is the panel's prompt, with the document fixed at open and the Actions paragraph.
  - `tools` is `AgentServer.catalogJSON()` minus `close_document`, each entry `{name, description, inputSchema}`.
  - `core_tools` is `AssistantHelperPrompt.coreTools`: `render`, `get_document`, `list_documents` and `open_document`.
  - `search_guide` and `compaction_instructions` come from `AssistantHelperPrompt`.
  - `settings` is `{auto_compact_tokens: int | null, clear_tool_results: bool}` (`AssistantChatSettings.helperJSON`; `null` never compacts).
  - `api_base` is `DR_ASSISTANT_BASE_URL` when set.
- **CallToolResult** is `{content?: [{type: "text", text} | {type: "image", data (base64), mimeType}], isError?}`.
  - The app splices `AgentServer.execute`'s JSON in raw. A result that is not valid JSON fails the whole line, and that call never resolves.
  - After a cancel, the app answers every request with `{"content":[{"type":"text","text":"Cancelled by the user."}],"isError":true}` without running it.

### Helper → app

| Event | Fields | The app |
| :--- | :--- | :--- |
| `ready` | — | ignores it; sent after `configure` |
| `turn_started` | — | counts itself busy, clears its cancel flag; the panel shows Stop |
| `text_delta` | `text` | streams the reply |
| `progress_delta` | `text` | streams a progress note (a thinking delta under display `updates`) |
| `context_usage` | `used` (int); `limit` (int or `null`) | updates the context meter |
| `tool_call` | `name`; `arguments` (object) | prints a meta line |
| `tool_request` | `call_id` (string, an increasing integer per process); `name`; `arguments_json` (string) | runs the tool on main and answers `tool_result` |
| `tool_result` | `name`; `is_error`; `summary` | prints `✗ <summary>` when `is_error` |
| `error` | `message` | prints it in red |
| `compaction_started` | `used` | prints a meta line |
| `compacted` | `before`; `after` | prints a meta line |
| `compaction_failed` | `reason` | prints a meta line |
| `tool_results_cleared` | `tool_uses`; `tokens` | prints a meta line |
| `turn_finished` | — | clears busy; always a turn's last event |

### A turn

- **Refusals with no turn events.**
  - A `send` before `configure` is answered `error` "The assistant helper was not configured."
  - A `send` during a turn is answered `error` "The assistant is busy."
- **The turn.**
  - `turn_started` comes first, then the text is appended to history.
  - Each round streams one request.
  - After each response come `context_usage` and one `tool_results_cleared` per applied `clear_tool_uses_20250919` edit.
- **Stop reasons.**
  - `tool_use` runs the tools, then sends the next round.
  - `pause_turn` and `compaction` resend to resume.
  - Any other reason ends the loop: the response is the turn's final reply.
  - A final reply's `tool_use` blocks (a `max_tokens` cut after a tool call) are never run and get no `tool_request`. Each is answered at once with an `is_error` tool_result, "Not run: the reply stopped with <stop_reason> before this call could run.", appended as a user message, because the API refuses every later request while a `tool_use` has no `tool_result` straight after it.
  - Thirty rounds also end it, silently.
- **At the end.**
  - Auto-compaction runs if due (below).
  - `max_tokens` then emits `error` "The reply was cut off at the output limit.", and `refusal` emits `error` "The model declined this request."
  - `model_context_window_exceeded` emits `error` "The reply was cut off because the conversation filled the model's context window."
  - `turn_finished` is always emitted, even after an error or a cancel.
- **Tools.**
  - For every `tool_use` block of a response at once, the helper emits `tool_call` and `tool_request`, then waits for every answer. The app runs the requests one at a time, in arrival order, each a separate hop to main under `AppActivity.userInitiated`.
  - Per answer, the helper emits its own `tool_result` and builds the Messages `tool_result` block:
    - Text becomes a text block.
    - An image becomes `{type: "image", source: {type: "base64", media_type: mimeType ?? "image/png", data}}`.
    - Other content types are dropped.
    - A falsy result (`null`, `false`, `0` or an empty string) is the error "The tool failed to execute.", and no usable content is the error "The tool returned no content."
    - `isError` gives `is_error: true` and keeps the blocks, images included.
  - The summary is the first text's first 200 UTF-16 code units, or "(image)".
  - A `tool_use` naming a tool not in `configure` is answered `Error: Tool '<name>' not found` (`is_error`) without a request.
- **Cancel.**
  - The in-flight stream is closed. Its partial reply is discarded and never enters history, and no `error` is emitted for it.
  - A pending tool wait is never resumed or cancelled. The app answers each request that arrives after the cancel with "Cancelled by the user.", so every `tool_use` in history keeps its `tool_result`.
  - No further request is sent. History may end with a user message of tool results the API never answered. The next `send` merges its text into that message instead of adding a second user message.
- **Errors.**
  - An API error with an HTTP status reads `API error <status>: <error.error.message, else the message>`.
  - A mid-stream SSE `error` event of a type Slaude maps reads the same with that type's status, from its own `error.message`, never `API error 200`.
  - An API error with no status reads `API error: <message>`: a connection that failed or timed out ("API error: Connection error."), a stream that ended before `message_stop` ("API error: The stream ended before its `message_stop` event."), and an `error` event of a type Slaude does not map.
  - Any other error reads its description.

### History, requests and context

- **History is append-only.**
  - Nothing sent is ever rewritten, and old renders stay in it.
  - Each assistant reply goes in exactly as received: thinking text and signatures, `server_tool_use` and tool-search results, compaction and fallback blocks, and fields Slaude does not model.
  - The one merge is the `send` merge into a trailing user message the API never answered: tool results left by a cancel, an error, the thirty-round cap or a final reply's unrun calls, or a user message whose request failed.
  - The one rewrite is compaction.
- **Requests:**
  - Each round is `POST /v1/messages` with `stream: true`. The body:
    - `model`, and `max_tokens` 32000.
    - `system`: one text block, `system`, a blank line, then `search_guide`, with `cache_control: {type: "ephemeral"}`: the explicit breakpoint that ends the static prefix (tools, then system), so a new conversation and the first request after a compaction read it instead of writing it again (measured live: such a first request wrote ~9.7K tokens without it).
    - `tools`: first `{type: "tool_search_tool_bm25_20251119", name: "tool_search_tool_bm25"}`, then the catalog in its order, each `{name, description, input_schema, defer_loading: true}`, with `defer_loading` absent for `core_tools`. The app's descriptions and schemas are kept as received, in their key order; the app writes every message with sorted keys (`AssistantHelperSession.write`), so the same catalog is the same bytes on every launch and the cached prefix survives a relaunch.
    - `messages`: the history.
    - `cache_control: {type: "ephemeral"}` at the top level, the automatic caching whose breakpoint moves to the newest block (the history); with the system block's marker that is two of the four breakpoints.
    - `output_config: {effort: "medium"}`, `thinking: {type: "adaptive", display: "updates"}` and `fallbacks: "default"`.
    - Only when `clear_tool_results` is on, `context_management: {edits: [{type: "clear_tool_uses_20250919", trigger: {type: "input_tokens", value: 150000}, keep: {type: "tool_uses", value: 5}, clear_at_least: {type: "input_tokens", value: 50000}}]}`. The Chat Settings sheet repeats the 150,000 in its wording.
    - The keys go in this order: `model`, `max_tokens`, `system`, `tools`, `messages`, `cache_control`, `output_config`, `thinking`, `fallbacks`, `context_management` (when on), `stream`. The summary request ends `stream`, `compaction`. The request-body tests pin the order, so the bytes of a conversation's requests stay the same in every process.
  - `anthropic-beta` is `thinking-display-updates-2026-08-18`, `server-side-fallback-2026-07-01` and `compact-2026-09-04`, plus `context-management-2025-06-27` while clearing is on.
  - The base URL is `api_base`, else `https://api.anthropic.com`.
  - `maxRetries` is 3 (Slaude's default is 2). Slaude's client core is the Python SDK's: it retries 408, 409, 429, 5xx (529 included) and connection errors, and honours `x-should-retry` and `retry-after`. A mid-stream error event is not retried.
- **Context meter:**
  - `used` is the last response's `input_tokens + cache_read_input_tokens + cache_creation_input_tokens + output_tokens`.
  - `limit` is `GET /v1/models/{model}`'s `max_input_tokens`, or `null` when it cannot be read. It is looked up after `configure`, which emits `context_usage` once it settles.
- **Compaction:**
  - **When.** Only at a turn's end: after a final reply (not `tool_use`, `pause_turn` or `compaction`) and not after a cancel. It runs once `used` is at least `auto_compact_tokens` (300,000 by default, range 20,000–900,000, `null` off). The turn's own settings decide it: a `settings` message during the turn applies from the next one.
  - **The skip.** When that final reply ended in `tool_use` blocks (answered unrun, above), the helper emits `compaction_started {used}`, sends no summary request, and after the loop emits `compaction_failed` "no summary came back"; the next turn end tries again.
  - **The request.** The helper emits `compaction_started {used}` and sends one summary request: the round's body without `context_management`, plus `compaction: {type: "summarize", instructions: compaction_instructions}`.
  - **Success** is `stop_reason == "compaction"` with a `compaction` block whose `content` is a non-empty string. History becomes that one response's assistant content, and `used` becomes the output tokens of its `usage.iterations` entry of type `compaction`. The helper emits `compacted {before, after}` and then `context_usage`.
  - **Failure.** Another stop reason emits `compaction_failed` "the summary stopped with <stop_reason>", and a `compaction` stop whose block has `null` or empty `content` emits "the summary came back empty". History and `used` are unchanged, and the next turn end tries again.

## Dropped

- **The TypeScript helper.**
  - `agent/src/main.ts` and `protocol.ts`, on `@anthropic-ai/sdk` 0.128.0's Tool Runner.
  - `agent/package.json`, `bun.lock` and `tsconfig.json`, and `agent/entitlements.plist` (`allow-jit`).
  - The Bun steps: `project.yml`'s Bun build phase, the Makefile's `bun test` `agent-test`, `publish.yml`'s `oven-sh/setup-bun` step, `.gitignore`'s `/agent/node_modules/`, and the README's Bun prerequisite.
- **Pricing (AH-4).**
  - `agent/src/pricing.ts`, `priceTable.ts` and their `bun test` suites.
  - `agent/prices/`: `anthropic.json` (the LiteLLM-derived table), `cases.json`, `reduce_cases.json` and `update.py`, the table's only writer.
  - `make prices`.
- **Protocol (AH-4).**
  - The events `prices`, `cost` and `message_done`.
  - `configure`'s `price_cache` and `price_feed`.
  - The host message `reset`.
  - The app's `assistant_text` case and its `appendAssistant`.
  - Partial-usage pricing of a cancelled response.

## Build and signing

- **The script.** `scripts/build_agent_helper.sh [DESTINATION]` is the one build:
  - `project.yml`'s "Build Agent Helper" post-build phase passes `Contents/Helpers/doctored-agent`.
  - `make agent` passes `build/agent/doctored-agent`, a copy for `DR_ASSISTANT_HELPER` or a stdio drive with no app build.
  - What it runs: `xcrun swift build --package-path agent --scratch-path build/agent-swift --configuration <DR_AGENT_BUILD_CONFIG> --product doctored-agent`, plus `-Xswiftc -no-whole-module-optimization -Xswiftc -enable-batch-mode` for release. It then removes the linker's ad-hoc signature, `strip -x`es into the destination, and signs it as AH-6 says. The driver's one `warning: ignoring -num-threads argument; cannot multithread batch mode` line (SwiftPM's release passes `-num-threads`) is dropped from the output, since Xcode would count it as a build warning.
  - `DR_AGENT_BUILD_CONFIG` is `release` unless set (`debug` is the other value). It is the same for every app configuration, so the app builds compile Slaude once, not once per app configuration. `make agent-test` (`swift test`, the gate's `agent-tests` step) compiles it once more in debug, on the same scratch path (60 s and a 3.7 GB tree peak, below).
  - **Why release compiles in batches, not as one whole module** (measured below): whole-module optimization of Slaude peaks at 10.6 GB in one compiler process, past the publish runner's ~7 GB, for a larger binary and JSON only 2-21% faster. In batches the build peaks at 2.1 GB with three jobs. Debug would be 1.6 times the size and 4-17 times slower at the history's JSON.
- **The scrubbed environment.** SwiftPM gets only `HOME`, `TMPDIR`, `DEVELOPER_DIR` (when set) and `PATH=/usr/bin:/bin:/usr/sbin:/sbin`. An Xcode phase exports the app target's `SDKROOT`, `TOOLCHAINS`, `SWIFT_*`, `MACOSX_DEPLOYMENT_TARGET` and the rest, which would steer SwiftPM's compiler.
- **The content stamp.** SwiftPM is skipped when a stamp beside the scratch path matches. The stamp is the digest of:
  - `agent/Package.swift`, `Package.resolved` and every file under `agent/Sources`;
  - the script;
  - the configuration and the toolchain;
  - the size and time of the product it made.

  A dependency whose source the digest cannot see always runs SwiftPM and keeps no stamp: one being edited in place (`swift package edit`, checked out under the ignored `agent/Packages/`), or a local path or git dependency, as `build/agent-swift/workspace-state.json` records them. A no-op SwiftPM build of Slaude's ~2,400 files takes 0.6-1.9 s; the stamp's digest takes 0.12 s, and the phase's no-op (digest, copy, strip, sign) 0.72-0.75 s.
- **The first build** fetches Slaude over the network at the pinned revision.
- **Phase order** in the generated project: Build Rust Core, Sources, Resources, Frameworks, Build Agent Helper, Build MCP Helper, Write Agent Catalog. Xcode's own CodeSign comes after them.
- **Publishing.** `publish.yml` needs no setup step: the archive compiles the helper through the phase, signs it with the Developer ID identity and a timestamp, and notarizes it with the app.
- **Measured** on 2026-11-03 on a 12-core, 96 GB Mac under load from other lanes (load average 20-490), each from an empty `build/agent-swift`, under the script's scrubbed environment (`/usr/bin/time -l` for the largest process, a `ps` sampler of the build's process tree for the sum). Parse and serialize are Slaude's `RawJSON.parse` and `serialized()` on a 2 MB history, minimum of 7 runs, for one dominated by a 1.9 MB base64 render and one of 1,937 small messages:

  | Build | Cold wall (CPU) | Largest process | Tree peak | Stripped | Render parse / write | Small-blocks parse / write |
  |---|---|---|---|---|---|---|
  | debug | 60 s (246 s) | 1.20 GB | 3.7 GB (12 jobs) | 55.9 MB | 87 / 71 ms | 240 / 107 ms |
  | release, whole module (SwiftPM's default) | 263 s (434 s) | 10.57 GB | 10.7 GB | 38.0 MB | 5.2 / 6.3 ms | 60 / 14 ms |
  | release, `-no-whole-module-optimization` | 183 s (1,489 s) | 1.85 GB | 3.2 GB (12 jobs) | 35.5 MB | 5.3 / 6.6 ms | 62 / 17 ms |
  | release, the same plus `-enable-batch-mode` (shipped) | 148 s (530 s) | 1.78 GB | 3.2 GB (12 jobs), 2.1 GB at `-j 3` (160 s) | 35.5 MB | the same code | |

  The bundled helper is 35.5 MB, where the Bun helper was 57.7 MB.

## Verification

- **`make agent-test`.** `swift test --package-path agent --scratch-path build/agent-swift`, with no network and dummy keys only, over Slaude's injectable transport serving scripted SSE and recording request bodies. The suite is the port's acceptance list:
  - the codec, every message both ways;
  - the tool-result conversion;
  - the `send` merge, the compaction accounting and the context arithmetic;
  - the busy and unconfigured refusals;
  - full turns: deltas; a multi-tool round trip with an image; a cancel mid-stream and one with tools pending; `refusal`, `max_tokens` and `model_context_window_exceeded`; a final reply's unrun tool calls answered, and the next request valid; 429/529 with `retry-after`; a mid-stream error event; `pause_turn`; compaction success, failure and an empty summary; settings changed mid-turn applying from the next turn;
  - byte-identical replay of thinking, signature and tool-search blocks in the next request body.
- **`make gate`:**
  - The `agent-tests` step runs `make agent-test`. Its inputs are `agent/Package.swift`, `Package.resolved`, `Sources` and `Tests`, the Makefile, the module cache check (`scripts/agent_module_cache.py` and `scripts/probe_build.py`, which the app leg's inputs also hold) and `scripts/gate.py`. It needs no core, so it starts beside both legs. It shares `build/agent-swift` with the app build's Build Agent Helper phase, and SwiftPM's lock on a scratch path serializes the two: whichever starts second waits for the first.
  - `launch`'s bundle check requires an executable `Contents/Helpers/doctored-agent` whose code directory has the `runtime` flag and that carries no entitlements (`gate.signing_failure`).
  - Once the app has stopped, `launch` runs that bundled helper through `scripts/verify_agent_helper.py --cases text-turn,framing` (`gate.helper_failure`): the copy that ships, built `-O` in batches, stripped and re-signed with the hardened runtime, where the suite runs SwiftPM's debug build.
  - The Swift leg's inputs name the helper's package and the build script.
  - `wait` waits for this checkout's `swift-build` and `swift-test` processes.
- **Freshness.** `scripts/app_under_test.py` refuses an app older than `agent/Sources`, `Package.swift` or `Package.resolved`.
- **`scripts/verify_agent_helper.py`**, the stdio conformance drive: it runs a built helper (the Debug bundle's by default, `--helper` for another) as the app does, under the app's scrubbed environment, against a loopback Messages API, with the bundle's catalog. Its 13 cases are text-turn, settings, tool-round-trip, cancel-stream, cancel-tools, refusal, max-tokens, context-window, rate-limit-retry, retries-exhausted, stream-error, two-processes and framing. The gate runs only text-turn and framing (above); the whole drive is the loop's check after a helper change.

- **`scripts/live_assistant_acceptance.py --live --yes-spend`**, the owner-approved acceptance run against the real API (it spends money; `--budget-usd`, default $5, stops it). A recording proxy between the helper and the API keeps every body and its usage, never a header; an owned instance runs the tools. Six scenarios: three tool turns, a relaunch, a cancel mid-stream, a cancel with tools pending, eight turns, and compaction at the app's 20,000-token minimum; `--self-test` runs them against the loopback stub. Refusal and `max_tokens` are left to the scripted cases. Run 2026-11-04 on the Release build (`build/live-acceptance/20261104T012253Z`), all six PASS, 36 requests, all 200, no error event, $1.18 at Opus 5.5 rates:
  - every request after a conversation's first read the cache, growing across turns (S1's turn-opening reads 0 → 23,006 → 34,724, the larger figures summing the API's tool-search iterations); eight turns read 82.3% of their input from it;
  - a relaunched helper's first request was byte-identical to the first one's and read 9,738 cached tokens;
  - both cancels left a history the next send was accepted on, with the cancelled text absent from it;
  - compaction took 50,294 tokens to 567 and the next turn was accepted.
  That run also showed each new conversation (and the first request after a compaction) writing the ~9.7K-token tools and system prefix again, since the automatic breakpoint sits only on the newest block; the system block's explicit breakpoint (above) fixes it: with it, a new conversation's first request read 9,638 tokens and wrote only its message's 100-132 (`--nonce --scenarios S3,S4`, `20261104T012832Z`).

## Accepted risks

- **Slaude is two days old and has a single maintainer**, Doctored's owner. Its transformation and ~3,800 tests track the official Python SDK, but no one else uses it yet. Its first CI run (macOS on Xcode 26.2 and the newest Xcode 26, Linux on Swift 6.4) passed on 2026-11-03.
- **Refresh cadence.** The pin moves only in its own reviewed commit, which runs `make agent-test` and the gate. A new API feature or fix reaches the helper only through such a bump. Slaude follows the Python SDK through `Tools/update-upstream.sh`.
- **The release compile on CI.** In batches, 435 s of CPU and a 2.1 GB peak at three jobs on a 96 GB Mac. On the publish runner (`macos-26`, ~7 GB, publish run 4, v0.4.4, 2026-11-04) SwiftPM built Slaude and the helper in 279.5 s with no memory failure, and the whole archive step took 20 min 2 s against 19 min 11 s for the last Bun build. If a later Slaude outgrows the runner, the levers are `DR_AGENT_BUILD_CONFIG=debug` and a larger runner.
- **Build cost.** Every lane's first app build compiles Slaude in release, minutes where Bun took about 0.2 s, and its first `make agent-test` compiles it again in debug. Later builds are incremental or skipped by the stamp.

## Open

Nothing is open.
