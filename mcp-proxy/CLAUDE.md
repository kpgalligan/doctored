# mcp-proxy/CLAUDE.md — the stdio MCP helper's rules

Applies to everything under `mcp-proxy/`, the crate (package `doctored-mcp`) that builds `doctored-mcp`:
the stdio MCP server bundled as `Doctored.app/Contents/Helpers/doctored-mcp`,
which external MCP clients start (decisions and traps in
`designs/agent-socket.md`, ported from Frame Job). `make mcp-test` runs its suite
and `make lint` clippies it. `project.yml`'s "Build MCP Helper" phase builds,
copies and signs it (`--options runtime`, no entitlements); "Write Agent
Catalog" then writes the snapshot it falls back to.

## Module map

One line per file; each file's `//!` doc and item docs hold its detail.

- `lib.rs` (and `main.rs`, which only calls it) — `run()`, the stdin loop; it includes
  `core/src/mcp_versions.rs` by `#[path]`, so helper and app negotiate from ONE list.
- `session.rs` — `dispatch`, `Output` and `Session`: what is answered here and what
  is forwarded, the `list_changed` check and the stranded session.
- `socket.rs` — `socket_path`, `is_scratch`, `reach` and `post` (one `POST /mcp`,
  its body read by `Content-Length` or chunked).
- `launch.rs` — `ensure_running`: the locked launch of its own bundle and the poll
  until the socket answers or `launch_wait()` ends; `open_arguments`, with the
  launch marker `HELPER_LAUNCH_ARGUMENT`.
- `bundle.rs` — `own_bundle` and `read_snapshot`, the catalog snapshot the build writes.

## Rules

- **stdout is protocol only**; stderr lines start `doctored-mcp: `.
  **`initialize`, `ping` and `tools/list` never launch Doctored.**
- **The launch lock is held until the poll ends**, so a second helper's re-check
  finds the first one's instance. **No read timeout on a forwarded call** (a save
  may take minutes); only the session's `tools/list` passes `LIST_READ_TIMEOUT`.
- **Every launch ends `--args … -DRMCPHelperLaunch YES`** (`HELPER_LAUNCH_ARGUMENT`,
  the app's `AppUpdater.helperLaunchArgument`): no person opened the instance, so
  a published build's updater waits for the first activation. The person's socket
  gets `-n -g <bundle> --args -DRMCPHelperLaunch YES` and nothing else
  (`designs/sparkle-updates.md`).
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
