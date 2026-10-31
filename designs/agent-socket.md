# The agent server on a Unix socket, and the `doctored-mcp` helper (ADR)

- **Status:** Accepted — closed; shipped 2026-10-14 in one batch
- **Source:** a port of Frame Job's road map 14 (MC1–MC5) and its decisions MC-1..MC-6, at the commit the batch's handoff recorded; Frame Job's ADR is the full record of the spike and its measurements.
- **Related:** client authorization, declined 2026-10-28 (see Known limits); the assistant helper's own port, unchanged

## Context

The agent server was streamable HTTP on `127.0.0.1:4816`, off until Tools ▸ Allow Agent Connections or `-AgentServerEnabled YES`, with an ephemeral-port fallback and Origin/Host/Content-Type admission against web pages. An MCP client lists tools once, when it starts, so a client opened before Doctored never saw its tools, and any local process that followed the admission rules could connect while it was on.

## Decisions (MC-1..MC-6, Doctored names)

- **MC-1 — socket only.** The agent server listens only on `~/Library/Application Support/Doctored/Agent/agent.sock`, always while the app runs. There is no TCP listener and no switch: `dr_agent_server_start/stop`, `DR_AGENT_PORT`, `AgentServerEnabled`, the port fallback and the menu item are deleted end to end. The `/mcp` path check (404) and POST-only (405) stay; Origin/Host/Content-Type admission went with TCP.
- **MC-2 — who binds** (`core/src/agent.rs` `prepare_socket_path`, moved from Frame Job's Swift into Rust so `core/tests/agent_tests.rs` tests it). The socket sits in its own `Agent` folder, made `0700`, which must be a real folder (lstat, not a symlink) owned by this user with no group or world bits; a wrong mode is reported, never worked around. A socket that accepts a connection belongs to another instance and is left alone; one that refuses is a crash's leftover and is unlinked; a file that is not a socket is left alone. The owner unlinks the socket at quit only while the file is still its own (device and inode, `SocketClaim`). A launch that cannot bind logs one `Doctored agent socket not bound: <reason>` line and carries on; it never alerts.
- **MC-3 — `doctored-mcp`.** External clients start the stdio helper bundled at `Contents/Helpers/doctored-mcp` (`mcp-proxy/`, std + serde_json; its rules are `mcp-proxy/CLAUDE.md`). Settings ▸ MCP shows its path and copies `claude mcp add --scope user doctored -- '<path>'`. The protocol versions are one file, `core/src/mcp_versions.rs`, which the helper includes by `#[path]`.
- **MC-4 — what launches.** Only a `tools/call` that finds no socket launches Doctored, and only the helper's own bundle; `initialize`, `ping` and `tools/list` never launch (the list falls back to the bundled snapshot). The mechanics are `mcp-proxy/CLAUDE.md`'s Rules (the launch lock held until the poll ends) and the `mcp-proxy/src/launch.rs` and `session.rs` module docs (the poll, the stranded session).
- **MC-5 — no modal question inside a tool call; a click still asks.** Doctored already had this, so no code changed: no `AgentServer*.swift` calls `NSAlert`, `runModal` or `beginSheetModal`; `open_document` opens every import headless; `performPixelEdit` drops a layer description silently where the UI asks; `close_document` refuses an unsaved document unless told to discard.
- **MC-6 — `DR_AGENT_SOCKET` overrides the path, for the app and the helper, and every test launch sets it** to a socket in a fresh `0700` folder under `$TMPDIR` (`scripts/mcp_client.py` `scratch_socket`). A scratch launch by the helper carries the scratch socket and the test environment to the app it starts (the arguments it passes: `mcp-proxy/src/launch.rs`'s module doc, `forwarded_environment`). Lanes need no MCP port ranges.

Also decided here: the catalog snapshot is written at build time (`project.yml` "Write Agent Catalog", after "Build MCP Helper") by an ad-hoc-signed run of the just-linked executable with `-DRWriteCatalog <path>` (`AgentCatalogSnapshot.swift`), which goes through the same catalog parity check as a launch; the bundle is then re-sealed with Xcode's identity, runtime and entitlements. Debug sets `ENABLE_DEBUG_DYLIB: NO` so that run works. The quit releases without a join (`dr_agent_socket_release`), so a call in flight that waits on main cannot hold the quit; the serve thread's stop machinery went with it.

## Traps (each cost Frame Job a measurement)

- `tiny_http` 0.12's `Server` drop connects to its path and unlinks it unconditionally, so a listener whose file was replaced would delete another instance's socket: never drop a listener whose claim is not current (`release` forgets it; the serve thread holds its own `Arc`).
- `server_addr().to_ip()` is `None` for a Unix listener.
- macOS's `sun_path` holds 104 bytes with the NUL, so a socket path may be 103 bytes; the product path is far below it, a socket under a repo work folder is not, which is why scratch sockets live under `$TMPDIR`.
- `tiny_http` answers a body of 32 KB or more chunked (a `tools/list` is about 900 KB): every client decodes chunked bodies (`mcp_client.Client` through `http.client`, the helper's own reader).
- Tool lists are compared as JSON values, never bytes: key order differs between processes (the core's serde maps sort keys; Swift's JSONSerialization does not). `generate_mcp_reference.py` renders from sorted keys, so `--socket` and `--catalog` give the same page.

## Known limits

- Any process running as the user can drive Doctored while it runs, and this cannot be turned off; web pages, other accounts and sandboxed apps cannot (client authorization was declined on 2026-10-28: such a process can read any token file and launch the bundled `doctored-mcp` itself, so the protection is the path guard and the socket's `0700` folder).
- The helper's launch of the real app is proved only by the fake bundle's recorded launch, and no drive launches the real app through the helper (the rule is in `mcp-proxy/CLAUDE.md`); `scripts/verify_mcp_helper.py` runs the helper only against an instance it started itself.
- `open -n` copies of a fake bundle stay registered with LaunchServices only until the fixture's `lsregister -u`.
- `DROPPED` (2026-10-30) — proving a quit with a call in flight: the jobs-probe "agent release mid-call" case covers the release path.

## Where it lives

`core/src/agent.rs`, `core/src/ffi_agent.rs`, `core/src/mcp_versions.rs`, `core/tests/agent_tests.rs`; `mcp-proxy/`; `app/Sources/AgentServer+Socket.swift`, `AgentServer+ServeThread.swift`, `DoctoredCore+AgentServer.swift`, `AgentCatalogSnapshot.swift`, `MCPSettingsPane.swift`; `scripts/mcp_client.py`, `scripts/verify_mcp_helper.py`, the jobs-probe's "agent release mid-call" and "mcp settings pane" cases.
