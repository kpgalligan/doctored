# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Per-side rules live next to the code they govern and are imported here, so every agent has them from its first turn:

@core/CLAUDE.md
@app/CLAUDE.md
@mcp-proxy/CLAUDE.md

The per-file indexes are `core/MODULES.md` (Rust modules) and `app/FILES.md` (Swift files, including which six are frozen): grep an index by file name before editing or adding a file (`grep -n -F 'Bitmap.swift' app/FILES.md | cut -c1-400`), and never read one whole.

## Commands

```sh
make app        # cargo build --release + xcodegen + xcodebuild → build/Build/Products/Release/Doctored.app (Release; once per batch at the final gate)
make app-debug  # cargo build --release + xcodegen + xcodebuild Debug → build/Build/Products/Debug/Doctored.app; the loop's Swift check and the app drives use
make test       # the whole Rust suite through scripts/run_tests.py: one build, the standalone test binaries four at a time with the serial suite beside them, the other suites one at a time, then the solo list one at a time; lists every failing binary
make t F=<file> T=<filter>  # one Rust test file, whichever binary holds it (scripts/run_test_file.py); T is optional; fails when no test runs
make test-serial  # the whole Rust suite as plain cargo (cd core && cargo test --release)
make gate       # the one Full verification command: the docs check, fmt, core build, clippy, the suite, the doctored-mcp suite, the Release app, a launch over a scratch socket, a type check of each changed probe directory (see "Verification")
make cancel-bench  # the ignored wall-clock probes (release, serial, no fail-fast); its output is the measurement record
make lint       # scripts/check_layers.py, then cargo clippy --all-targets -- -D warnings (core/, then mcp-proxy/)
make agent-test # the assistant helper's tsc and bun test (agent/; no network)
make mcp-test   # the doctored-mcp stdio helper's suite (mcp-proxy/; make lint clippies it, make gate runs both)
make prices     # NETWORK: regenerate agent/prices/anthropic.json from LiteLLM; its only writer
make typecheck  # fast type check, no artifact, stops before SILGen
python3 scripts/probe_build.py --typecheck scripts/<probe dir>  # the same check of the app sources with one probe directory's Swift files; the loop check for a probe edit
make project    # regenerate Doctored.xcodeproj from project.yml (injects VERSION; never run bare xcodegen)
make run        # build and launch
```

- Single Rust test: `make t F=selection_tests T=magic_wand` (`F` is the test file's name; `T` is the start of the test's name, `module::name` for a test in an inline module). A filter that matches no test fails. Run `cargo fmt` in `core/` and keep `make lint` clean before finishing Rust work.
- SourceKit/IDE per-file Swift diagnostics are **noise** — they lack the bridging-header context and report false errors.
- `make app-debug` is the loop's Swift check, and it builds the app that drives use.
- `make typecheck` is a fast check with no artifact that stops before SILGen.
- `make app` (Release) runs once per batch at the final gate.
- Do not run `make typecheck` when an app build follows.
- `Doctored.xcodeproj` is generated; `project.yml` (xcodegen) is the source of truth. Never commit the project file. Both xcodegen and `make typecheck` glob `app/Sources/`, so a new Swift file needs no registration anywhere. Target is arm64, macOS 15 (Sequoia) — raised from 13 for Vision's `VNGenerateForegroundInstanceMaskRequest` (subject lifting), which is macOS 14+. The floor is deliberately generous: prefer the modern API over a back-compat bridge, and don't add `if #available` gates below 15.

## Architecture

Swift/AppKit UI (programmatic, no storyboards) over a Rust staticlib core. All pixel work happens in Rust; Swift owns windows, tools, undo, and agent tool dispatch. Design decisions live in `designs/` as condensed ADRs, one per phase or study. The feature catalogs are `gimp-image-manipulation-capabilities.md`, `high-leverage-roadmap.md`, `next-features.md` and `photo-editor-gap-review.md`; check them before designing a feature from scratch. The live working docs are `professional-photo-editor-implementation-plan.md` (the roadmap), `phase-7-carryovers.md` (the carryover ledger), and the status docs of any phase still open. Open items in the ADRs link their GitHub issues.

Photoshop is named only as a compatibility target and a test reference: PSD, ASL and preset-file exchange, the `scripts/photoshop-probe/` takes, and the matches a take measured. It is never the model for a feature or the reason for a design. Code comments, docs, catalog strings, commit messages and issue text state Doctored's own rule; they never give "as in Photoshop", "like Photoshop's", "Photoshop parity" or "Photoshop users expect" as a reason. Public releases are snapshots that `scripts/public_snapshot.py` makes of a ref, minus the paths `PUBLIC_EXCLUDE` lists; both stay out of the public repository, as does `scripts/photoshop-probe/`. No kept file may import or require an excluded path or hold an absolute home path (`/Users/` or `/home/` followed by a user name, or its dash-encoded form `-Users-<name>-`; write `~/...` or a placeholder), and `python3 scripts/public_snapshot.py --dry-run` checks both.

Track open work as GitHub issues (`gh issue create -R kpgalligan/doctored-dev`), not as todo lists in local docs. A small fix whose right answer is clear (a stale comment or doc line, a wrong name, a typo, a few obvious lines) is not open work: make it in the change you are on, even outside its stated scope, and say so in the commit message. When you find unfinished work that needs a decision, a design, a measurement or more than a few minutes (a follow-up, a known gap, a deferred item), file an issue with the `ai-drafted` label, plus `low-priority` when it is, and link it from any doc that mentions it (`[#N](https://github.com/kpgalligan/doctored-dev/issues/N)`). The tree cites an issue only while it is open: never as the source or history of shipped code, in a comment, doc, catalog string, test or drive. The batch that closes an issue removes every reference to it from the tree; its commit message keeps the number.

`/archive` is historical. Don't consult it unless asked, or unless you need to know why an earlier phase made a particular decision. The index is `/archive/INDEX.md`.

### Status docs

- A status doc (`designs/*-status.md`) is the working record of an **open** phase and holds only what a later batch needs: decisions in force, the contract of each sub-slice not yet shipped, open items linked to their issues, and for each shipped sub-slice a few lines (what shipped, the commit, anything a later slice must know). The batch that changes any of those updates it.
- Run logs, receipts, measurement tables, review histories and superseded designs do not go in it. They stay in the handoff directory and the receipts folders outside the repo, or in the commit message. The one exception is a table the phase's plan requires the doc to hold, headline rows only, each number with its command.
- When a sub-slice ships, its section is cut to those few lines in the same batch. When a phase closes, the closing batch condenses the doc into an ADR at the same basename, moves the original to `archive/` unedited (`git mv`; into `archive/<YYYY-MM>/` when `archive/` already holds that basename), and adds its line to `archive/INDEX.md`.
- An open phase's status doc may have a shipped companion, `designs/<same basename with -status replaced by -shipped>.md`, made when the settled record of shipped sub-slices takes the status doc past about 24 KB; whole sections move verbatim. It holds only the settled record: each shipped sub-slice's few lines and the decisions in force. Nothing the second bullet excludes goes in it. The status doc keeps the head, the plan, the contract of each sub-slice not yet shipped, open items, and a one-line `## Shipped runs` section linking the companion. Once it exists, the batch that ships a sub-slice puts that sub-slice's few lines and decisions there. The closing batch condenses both into the one ADR and archives both originals.
- The same holds for the ADR of a shipped phase: a batch that closes an issue against it adds a few lines, not its record.
- A status doc stays under 40 KB and a shipped companion under 32 KB. `scripts/check_docs.py`, a step of `make gate`, fails on a larger one of either, on `core/CLAUDE.md` over 8,000 bytes or `app/CLAUDE.md` over 16,000, and on any `core/MODULES.md` or `app/FILES.md` entry line over 200 characters.
- The roadmap and the carryover ledger are live working docs, not status docs; the cap and the check do not apply to them.

### README

- A batch does not edit `README.md`, except to fix a statement in it that the batch makes false; that fix goes in the same batch.
- A periodic sweep brings it up to date. `python3 scripts/readme_sweep.py` lists the commits since the marker (README's last line, `<!-- readme-swept-to: <sha> -->`) with the `designs/` docs each changed. The sweep reads each commit's message (`git show -s <sha>`) and its status-doc entry, checks the MCP tool list against a running app's scratch socket or a built bundle's catalog (`python3 scripts/generate_mcp_reference.py --check --socket <DR_AGENT_SOCKET>` or `--check --catalog build/Build/Products/Debug/Doctored.app/Contents/Resources/agent-catalog.json`; regenerate the page when it is stale), edits README only where those commits changed what it says, and moves the marker to the last commit it read, never to a placeholder for its own commit.
- The sweep runs before every push to `publish` and at least every 20 commits (`python3 scripts/readme_sweep.py --count`).

### FFI contract

`core/include/doctored_core.h` is a **hand-maintained** C header — the single contract between the worlds (imported via `app/Bridging/Doctored-Bridging-Header.h`). Changing any `dr_*` signature means editing the Rust `ffi_*.rs` file, the header, and the Swift wrapper (the area's `app/Sources/DoctoredCore+<Area>.swift`; `DoctoredCore.swift` holds the handle classes) in lockstep. Conventions, uniform across the surface:

- Opaque handles: `DrImage` (one image) and `DrDocument` (canvas + bottom-first layer stack). Pixels are straight (non-premultiplied) RGBA8, row 0 = top.
- **All operations are pure** — they return a new handle and never mutate the input. Undo/redo on the Swift side is therefore just a stack of handles (`levelsOfUndo` is 0; retention is bounded by the per-document history byte budget — `set_history_budget`, Phase 5).
- Every FFI function wraps its body in `catch_unwind`, tolerates NULL inputs, and reports errors through an `err_out` heap CString freed with `dr_string_free`. Strings returned to the host come from `dr_agent_string_create`-style `CString::into_raw` and are freed by the matching free function.
- Legacy selections cross the boundary as canvas-sized u8 coverage masks (0 outside, 255 inside, intermediate = anti-aliased edge). The additive native selection and precision-document APIs use u16 coverage (0…65535); `CanvasSelection.maskSamples16()` retains those exact samples, while `maskBytes()` is the explicit byte adapter. The existing byte FFI is unchanged. Byte painting strokes use a canvas-sized **premultiplied** RGBA8 overlay handed to `dr_doc_painting_layer`; typed operations declare their separate native coverage inputs in the header.

### Where a feature lands — the standard recipe

Every shipped feature follows the same shape (visible in each feature commit); putting a step's code anywhere else is what bloats files:

1. Pure model + ops in a **new Rust module** with its own `impl DrDocument` block (the `doc_select.rs` / `doc_transform.rs` pattern), plus per-area tests in a new `core/tests/*.rs` file, mounted in its family's suite root (`core/tests/suite_*.rs`).
2. FFI three-file lockstep: `ffi_*.rs` shim → header → `DoctoredCore+<Area>.swift`.
3. Swift model/value types + `extension RasterDocument` in a **small dedicated file** (the `TextLayer.swift` / `LayerTransform.swift` / `AdjustmentLayer.swift` pattern).
4. Canvas gesture handling as a session struct plus `on*` closures in `ImageCanvasView` — wiring only, logic in the feature's file.
5. Controller logic in an `EditorViewController+<Feature>.swift` extension file; menu items in the menu's `AppDelegate+<Menu>Menu.swift` builder (nil-target selectors through the responder chain; `AppDelegate.buildMainMenu()` orders the top-level menus); validation cases in `validateUserInterfaceItem`.
6. MCP parity: an `AgentServer+<Feature>.swift` handler and its entry in that file's handler table and its `ToolFacts` entry (recording, selection, depth admission) in that file's `<prefix>ToolFacts` table (listed in `AgentServer+Registry.swift`) + catalog entry (see `app/CLAUDE.md`), with a comment naming the UI path it mirrors.
7. Dialogs as a `…SheetController` built on the shared `makeSheetView` builders in `Sheets.swift`.
8. Verify as "Verification" below says. README is not edited per batch (see "README").

## Verification

- In the loop: targeted `make t F=<file> T=<filter>`, `make lint` after Rust edits, `make app-debug` after Swift or header edits, and the drives the change needs, against the Debug app.
- After an edit to a probe's Swift (`scripts/<probe dir>/*.swift`): `python3 scripts/probe_build.py --typecheck scripts/<probe dir>`. It stops before SILGen, like `make typecheck`; the probe's runner is what builds and runs it. The runners that build through `scripts/probe_build.py` keep the linked probe in `build/probe-cache`, so a second run on unchanged sources compiles nothing. `DR_PROBE_BUILD_ONLY=1` ends a runner that builds through `scripts/probe_build.py` right after the build, before it launches anything; that is how a `scripts/*_ui.py` drive's build is checked.
- Once, when the code is complete and before review: `make gate`. It is the only Full verification command; a spec lists it once, plus any Release-only drive the batch needs.
- After a fix pass: `make gate` again. It re-runs only the legs whose inputs changed.
- Never run the whole suite, `make typecheck` before an app build, or a gate step by hand as a substitute; never run a check twice to see its output differently.
- When a refusal or error string changes, grep `core/tests` for the old wording before running anything.

`make gate` ends with the `probes` step: the same type check of each probe set whose directory under `scripts/` has uncommitted changes, and of each set whose last type check failed, until it passes. The step fails on a `scripts/` directory that holds Swift and is in neither `PROBE_SETS` nor `STANDALONE_DIRS` of `scripts/probe_build.py`.

`make gate` takes `GATE_ARGS="--force"` to run every step; the probes step still covers only probe directories that changed or failed last time, so a committed probe directory is checked with `python3 scripts/probe_build.py --typecheck scripts/<probe dir>`. It returns non-zero for any outcome but a pass, and its last line says which: `gate: FAIL (<steps>)` names the failing steps (`(gate)` means the script itself raised, with the traceback on stderr); `gate: BLOCKED ...` means another build, test run or gate in this checkout was running and nothing ran, so run it again when that one ends. Logs are `build/gate-logs/<step>.log`.

### Parallel worktrees

- Batches run in parallel lanes: the main checkout and git worktrees under `.claude/worktrees/lane-*`. Each lane builds its own `core/target` and `build/`.
- `make gate` holds its own checkout's `build/gate.lock` and waits only for builds, test runs and gates in that checkout, never another lane's.
- The Photoshop probe lock (`run_photoshop.probe_lock()`, a machine-wide `flock`; see `scripts/photoshop-probe/README.md`) serializes Photoshop takes across lanes.
- Wall-clock measurements (`make cancel-bench`, the time-measuring Release-only drives) need the machine idle: run them only when no other lane is building.
- Lanes need no MCP port ranges: the agent server listens only on a Unix socket, and every owned instance gets a private scratch socket of its own (`mcp_client.scratch_socket()`), so two lanes' instances never meet.

## Driving a built app for verification

The MCP server is the reliable way to exercise the app end-to-end (open documents, run tools, render the canvas as PNG, check undo). It listens only on a Unix socket (`designs/agent-socket.md`): `~/Library/Application Support/Doctored/Agent/agent.sock`, which no test may bind, or the path `DR_AGENT_SOCKET` names. Every test launch sets `DR_AGENT_SOCKET` to a scratch socket of its own (a fresh `0700` folder under `$TMPDIR`, for macOS's 103-byte socket path) and is a child of the script that started it:

```python
import os, subprocess
from mcp_client import Client, scratch_socket, wait_ready   # scripts/mcp_client.py
socket = scratch_socket()
app = subprocess.Popen(
    ["./build/Build/Products/Debug/Doctored.app/Contents/MacOS/Doctored", "-ApplePersistenceIgnoreState", "YES"],
    env=dict(os.environ, DR_BACKGROUND="1", DR_AGENT_SOCKET=str(socket), ANTHROPIC_API_KEY="dummy",
             DR_WORKING_LIMIT_BYTES="2147483648"))
client = Client(socket); wait_ready(client, app, 30, "app log")
# client.call("list_documents") ...; then app.terminate() in a finally
```

`scripts/mcp_client.py` is the one client every drive and launcher uses (HTTP over the socket, chunked bodies included); `phase5_support.App`, `verify_phase1.instance` and the drives' own `App` classes wrap the lines above. `curl --unix-socket <socket> -H 'Content-Type: application/json' --data-binary @- http://localhost/mcp` reaches it from a shell. External clients use the bundled stdio helper `Contents/Helpers/doctored-mcp` (`mcp-proxy/`; Settings ▸ MCP copies its `claude mcp add` line); `scripts/verify_mcp_helper.py` drives it against an owned instance.

Owned test instances pin the working limit with `DR_WORKING_LIMIT_BYTES=2147483648`, so admission and refusals match the 2 GiB reference limit on every machine instead of following its RAM.

The assistant's API key lives in `~/Library/Application Support/Doctored/anthropic_api_key` (the env var wins over it; the keychain is legacy, read once only to migrate). Always launch test instances with a dummy `ANTHROPIC_API_KEY` anyway: it short-circuits every stored-key read, so a test instance can never pick up the user's real key — and on a machine where only the legacy keychain copy exists, it also avoids blocking launch behind that one migration prompt.

`DR_BACKGROUND=1` makes the instance an `.accessory` app (no Dock icon, no menu bar) that never activates itself and orders its windows behind other apps' windows while it is inactive — set it for every test launch (`app/Sources/BackgroundLaunch.swift`). `export DR_BACKGROUND=1` once reaches every `scripts/` launcher with no edits, because all of them build the child env from `dict(os.environ, …)` / `os.environ.copy()` (the real-app launchers: `phase5_support.App` and its `host-source-probe/support.py` `HostApp` subclass, `verify_phase1.instance`, `verify_phase9i_assisted.instance`). The `scripts/*_ui.py` drives compile their own probe `main.swift` and activate themselves on purpose (their synthesized key events need a key window), so they still come to the front with the variable set; setting it does not change their outcome (`scripts/verify_phase9d_ui.py` gives the same result with and without it). A background instance is not active, so an MCP call that omits `document_id` cannot rely on a main window — pass `document_id` when more than one document is open.

`scripts/` drives default to the Debug app through `scripts/app_under_test.py`; `DR_VERIFY_APP` selects another app; the launchers refuse a binary older than the Swift sources, the header, the core library or the `doctored-mcp` sources and name the `make` target; `DR_ALLOW_STALE_APP=1` overrides.

Automated runs never show a modal alert or error (`app/Sources/AutomationAlerts.swift`): under `DR_BACKGROUND=1`, or `DR_AUTOMATION_ALERTS=record` (which every `scripts/` probe runner and launcher sets), each `NSAlert` is answered with its first button and each error presentation as "not recovered", and closing a dirty document is cancelled, never saved or discarded. Each is logged as one `[AutomationAlerts]` stderr line, so send a drive's stderr to a file and grep it. File panels and `ModalCardWindowController` dialogs are not suppressed.

Use a scratch `DR_AGENT_SOCKET` and argv-style defaults so nothing leaks into the user's own instances. The user often has their own Xcode-launched Doctored running — never signal or kill Doctored processes you didn't launch; find your own with `pgrep -f '^\./build/.*MacOS/Doctored'`. Prefer opening documents via the `open_document` MCP tool over launch-time argv paths.

### Release-only drives

These are not run against the Debug app. The app drives among them run with `DR_VERIFY_APP=build/Build/Products/Release/Doctored.app` after `make app`.

- `scripts/verify_phase4.py` — measures time.
- `scripts/phase4_raw_latency.py` — measures time.
- `scripts/raw-decode-probe/measure.py` — measures time.
