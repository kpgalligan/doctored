# mcp-proxy/CLAUDE.md — the stdio MCP helper's rules

Applies to everything under `mcp-proxy/`, the crate (package `doctored-mcp`) that builds `doctored-mcp`:
the stdio MCP server bundled as `Doctored.app/Contents/Helpers/doctored-mcp`,
which external MCP clients start (decisions and traps in
`designs/agent-socket.md`, ported from Frame Job). `make mcp-test` runs its suite
and `make lint` clippies it. `project.yml`'s "Build MCP Helper" phase builds,
copies and signs it (`--options runtime`, no entitlements); "Write Agent
Catalog" then writes the snapshot it falls back to.

## Module map

- `lib.rs` (and `main.rs`, which only calls it) — the modules and `run()`: the stdin loop
  (one `Session`, one `Output`; stdin's end exits at once). It includes
  `core/src/mcp_versions.rs` by `#[path]`, so helper and app negotiate from ONE list.
- `session.rs` — `dispatch` (blank line skipped; unparsable `-32700` and a batch
  `-32600` with a null id; notifications and responses dropped; each request on
  its own thread), `Output` (one line per message under one lock) and `Session`.
  `initialize` and `ping` are answered here; `tools/list` from the app within
  `LIST_READ_TIMEOUT`, else the snapshot, never launching; `tools/call` launches
  its own bundle on `Absent`; anything else is forwarded, else `-32601`. A
  helper-side call failure is a CallToolResult with `isError: true`. The
  `list_changed` check compares `serde_json::Value`s, never bytes. A launch that
  timed out STRANDS the session: later `Absent` calls launch nothing.
- `socket.rs` — `socket_path` (`DR_AGENT_SOCKET` when not empty, else
  `$HOME/Library/Application Support/Doctored/Agent/agent.sock`), `is_scratch`,
  `reach`, and `post`: one `POST /mcp` with `Connection: close`, its body read by
  `Content-Length` or CHUNKED (`tiny_http` 0.12 chunks bodies of 32 KB or more).
- `launch.rs` — `ensure_running`: folder made `0700`, `agent.launch.lock` beside
  the socket taken with `File::lock`, the re-check, `/usr/bin/open` with
  `open_arguments` (`-n -g`; a scratch socket adds `--env DR_AGENT_SOCKET=…`, one
  `--env` per `forwarded_environment` pair — every `DR_*` and
  `ANTHROPIC_API_KEY` —, `--stderr <folder>/doctored-launch.log` and `--args
  -ApplePersistenceIgnoreState YES`), and the 50 ms poll until the socket answers
  or `launch_wait()` (`DR_MCP_LAUNCH_WAIT`, else 60 s) ends in `timeout_sentence`.
- `bundle.rs` — `own_bundle` (the executable must be
  `<X>.app/Contents/Helpers/<name>`) and `read_snapshot`
  (`Contents/Resources/agent-catalog.json`, `{version, tools}`), written by
  `app/Sources/AgentCatalogSnapshot.swift` (`-DRWriteCatalog`).

## Rules

- **stdout is protocol only**; stderr lines start `doctored-mcp: `.
  **`initialize`, `ping` and `tools/list` never launch Doctored.**
- **The launch lock is held until the poll ends**, so a second helper's re-check
  finds the first one's instance. **No read timeout on a forwarded call** (a save
  may take minutes); only the session's `tools/list` passes `LIST_READ_TIMEOUT`.
- **A helper launches only its OWN bundle**; outside one a call answers the
  sentence naming the executable. `#![forbid(unsafe_code)]` stays in `lib.rs` and
  `main.rs`; every `src/` file stays under 400 lines. Dependencies: `serde_json` and `std`; dev `tiny_http`
  0.12, the app's own transport, so a stub chunks exactly as the app does.
- Tests are black-box in `tests/stdio.rs`: the built binary copied into a fake
  bundle with a snapshot and a scratch `DR_AGENT_SOCKET`; a stub is `tiny_http`'s
  `Server::http_unix`. Only a `Fixture::launchable` fake bundle may reach
  `ensure_running`: its script records its launch, environment and arguments and
  exits without binding, and its `Drop` runs `lsregister -u`. Any other
  `tools/call` from a fake bundle is made only while a stub answers.
- The real `open` launch path is proved only by the recorded-launch case
  (`a_scratch_launch_forwards_the_environment_and_arguments`), and no drive
  launches the real app through the helper.
