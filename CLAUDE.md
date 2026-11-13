# CLAUDE.md

Per-side rules live next to the code they govern and are imported here, so every agent has them from its first turn:

@core/CLAUDE.md
@app/CLAUDE.md
@mcp-proxy/CLAUDE.md
@agent/CLAUDE.md

The per-file indexes are `core/MODULES.md` (Rust modules) and `app/FILES.md` (Swift files, including which six are frozen): grep an index by file name before editing or adding a file (`grep -n -F 'Bitmap.swift' app/FILES.md | cut -c1-400`), and never read one whole.

## Commands

```sh
make app        # cargo build --release + xcodegen + xcodebuild → build/Build/Products/Release/Doctored.app (Release; once per batch at the final gate)
make app-debug  # cargo build --release + xcodegen + xcodebuild Debug → build/Build/Products/Debug/Doctored.app; the loop's Swift check and the app drives use
make rust       # cd core && cargo build --release: the release core archive (core/target/release/libdoctored_core.a) the probes compile against
make test       # the whole Rust suite through scripts/run_tests.py: one build, the standalone test binaries four at a time with the serial suite beside them, the other suites one at a time, then the solo list one at a time; lists every failing binary
make t F=<file> T=<filter>  # one Rust test file, whichever binary holds it (scripts/run_test_file.py); T is optional; fails when no test runs
make test-serial  # the whole Rust suite as plain cargo (cd core && cargo test --release)
make gate       # the one Full verification command: the docs check (doc sizes and home paths), the release tools' selftests, fmt, core build, clippy, the suite, the doctored-mcp suite, the doctored-agent suite, the Release app, a launch over a scratch socket with the Release bundle's updater check (R1–R5) and a stdio run of its doctored-agent, the probe runs (`PROBE_RUNS` in `scripts/gate.py`: nine probe runners' small runs), a type check of each changed probe directory (see "Verification")
make release-check  # the release tools' selftests (scripts/appcast.py, scripts/updater_check.py): no build, no network, no key; the gate's `release` step
make dist       # after `make gate` passes, never while it runs: the Distribution build the publish workflow archives (Release plus Sparkle; ad hoc, never launched as built), then scripts/updater_check.py over it and Release (its D9 launches a re-signed scratch copy under automation) and its mutants; DIST_ARGS passes build settings
make cancel-bench  # the ignored wall-clock probes (release, serial, no fail-fast); its output is the measurement record
make lint       # scripts/check_layers.py, then cargo clippy --all-targets -- -D warnings (core/, then mcp-proxy/)
make agent      # the assistant's helper alone (scripts/build_agent_helper.sh, the app build's phase) → build/agent/doctored-agent, stripped and signed as the bundle's copy; DR_AGENT_BUILD_CONFIG=release|debug (release by default)
make agent-test # the doctored-agent helper's suite (swift test in agent/ on the build/agent-swift scratch path; no network, no real key; make gate runs it)
make mcp-test   # the doctored-mcp stdio helper's suite (mcp-proxy/; make lint clippies it, make gate runs both)
make typecheck  # fast type check, no artifact, stops before SILGen
python3 scripts/probe_build.py --typecheck scripts/<probe dir>  # the same check of the app sources with one probe directory's Swift files; the loop check for a probe edit
make project    # regenerate Doctored.xcodeproj from project.yml (injects VERSION; never run bare xcodegen)
make run        # build and launch
```

- Single Rust test: `make t F=selection_tests T=magic_wand` (`F` is the test file's name; `T` is the start of the test's name, `module::name` for a test in an inline module). Run `cargo fmt` in each crate you edited (`core/`, `mcp-proxy/`; the gate's `fmt` step checks both) and keep `make lint` clean.
- SourceKit/IDE per-file Swift diagnostics are **noise** — they lack the bridging-header context and report false errors.
- `Doctored.xcodeproj` is generated; `project.yml` (xcodegen) is the source of truth. Never commit the project file. Both xcodegen and `make typecheck` glob `app/Sources/`, so a new Swift file needs no registration anywhere. Target is arm64, macOS 15 (Sequoia) — raised from 13 for Vision's `VNGenerateForegroundInstanceMaskRequest` (subject lifting), which is macOS 14+. The floor is deliberately generous: prefer the modern API over a back-compat bridge, and don't add `if #available` gates below 15.

## Architecture

Swift/AppKit UI (programmatic, no storyboards) over a Rust staticlib core. Pixel math lives in Rust. Swift decodes what the core cannot (ImageIO stills, CIRAWFilter RAW, AVFoundation frames), lays out text and runs the Vision and Core ML models, then hands the core their pixels; `Bitmap.swift` is the one place a CoreGraphics premultiplied buffer becomes the straight RGBA8 the FFI takes. Swift also owns windows, tools, undo and agent tool dispatch. Beside them: `mcp-proxy/` (the bundled `doctored-mcp` stdio helper), `agent/` (the built-in assistant's helper `doctored-agent`, a Swift executable on the Slaude Claude API client: `designs/assistant-helper.md`) and `public-docs/` (the Astro user-documentation site, with its own `CLAUDE.md`; it holds the feature reference and the generated MCP tool reference).

Design decisions live in `designs/` as condensed ADRs, one per phase or study. Before designing a feature from scratch, check the feature catalogs, the two gap reviews `designs/photo-editor-gap-review.md` and `designs/photo-editor-gap-review-2026-09.md`; `designs/gimp-image-manipulation-capabilities.md` is the mechanism reference. The live working docs are the roadmap (`designs/professional-photo-editor-implementation-plan.md`, a short phase index plus the open work, each item linking its issue) and the status docs of the phases still open (13B and CMYK); the Phase 7 carryover ledger (`designs/phase-7-carryovers.md`) is a closed record whose open rows are issues. Two ADRs keep one section current: the high-depth support matrix in `designs/phase-7-operation-support.md` and the model survey in `designs/neural-models.md`.

Photoshop is named only as a compatibility target and a test reference: PSD, ASL and preset-file exchange, the measurement takes, and the matches a take measured. It is never the model for a feature or the reason for a design. Code comments, docs, catalog strings, commit messages and issue text state Doctored's own rule; they never give "as in Photoshop", "like Photoshop's", "Photoshop parity" or "Photoshop users expect" as a reason. Development happens in this public repository (`kpgalligan/doctored`), so every commit, commit message and issue is public. No file in the tree, tracked or untracked and not ignored, may hold an absolute home path (`/Users/` or `/home/` followed by a user name, or its dash-encoded form `-Users-<name>-`); write `~/...` or a placeholder. `make gate` checks this (`scripts/check_docs.py`). The history before 6b0fef4f (its commit hashes, older issue numbers and the Photoshop measurement harness, `scripts/photoshop-probe/`) lives in the private archive `kpgalligan/doctored-dev`. A commit hash in a doc that this history lacks is from there.

Track open work only as GitHub issues (`gh issue create`), never as todo lists or untracked open items in local docs: every open item in an ADR or a status doc links its issue. A small fix whose right answer is clear (a stale comment or doc line, a wrong name, a typo, a few obvious lines) is not open work: make it in the change you are on, even outside its stated scope, and say so in the commit message. When you find unfinished work that needs a decision, a design, a measurement or more than a few minutes (a follow-up, a known gap, a deferred item), file an issue with the `ai-drafted` label, plus `low-priority` when it is, and link it from any doc that mentions it (`[#N](https://github.com/kpgalligan/doctored/issues/N)`). The tree cites an issue only while it is open; work shipped under an open umbrella issue may cite it. The batch that closes an issue removes every reference to it from the tree (comment, doc, catalog string, test or drive); its commit message keeps the number.

`/archive` is historical. Don't consult it unless asked, or unless you need to know why an earlier phase made a particular decision. The index is `/archive/INDEX.md`.

### Status docs

- A status doc (`designs/*-status.md`) is the working record of an **open** phase and holds only what a later batch needs: decisions in force, the contract of each sub-slice not yet shipped, open items linked to their issues, and for each shipped sub-slice a few lines (what shipped, the commit, anything a later slice must know). The batch that changes any of those updates it. A closed phase's ADR usually keeps the `-status.md` basename, so the glob also matches ADRs, and the size cap below covers both.
- Run logs, receipts, measurement tables, review histories and superseded designs do not go in it. They stay outside the repo (the workflow's handoff files and the receipts folders under `~/devel/doctored-fixtures/`), or in the commit message. The one exception is a table the phase's plan requires the doc to hold, headline rows only, each number with its command.
- When a sub-slice ships, its section is cut to those few lines in the same batch. When a phase closes, the closing batch condenses the doc into an ADR at the same basename, or under the name the closing request gives (`designs/grayscale-mode.md` from `grayscale-mode-status.md`), moves the original to `archive/` unedited (`git mv`; into `archive/<YYYY-MM>/` when `archive/` already holds that basename), and adds its line to `archive/INDEX.md`. `scripts/verify_status_condense.py` checks the condensation (`--record` names an ADR under another name).
- An open phase's status doc may have a shipped companion, `designs/<same basename with -status replaced by -shipped>.md`, made when the settled record of shipped sub-slices takes the status doc past about 24 KB; whole sections move verbatim. It holds only the settled record: each shipped sub-slice's few lines and the decisions in force. Nothing the second bullet excludes goes in it. The status doc keeps the head, the plan, the contract of each sub-slice not yet shipped, open items, and a one-line `## Shipped runs` section linking the companion. Once it exists, the batch that ships a sub-slice puts that sub-slice's few lines and decisions there. The closing batch condenses both into the one ADR and archives both originals. An ADR whose open items keep shipping may keep its shipped companion under the same rules. An open phase whose shipped companion reaches its cap condenses that settled record into an ADR at the closing name (CMYK's is `designs/cmyk-mode.md`), archives the companion unedited with its `archive/INDEX.md` line (checked by `scripts/verify_status_condense.py --archive-dir archive --record <ADR> <companion>`), and starts a new companion only when the status doc next needs one; the closing batch then condenses the status doc and any new companion into that ADR.
- The same holds for the ADR of a shipped phase: a batch that closes an issue against it adds a few lines, not its record.
- A status doc stays under 40 KB and a shipped companion under 32 KB. `scripts/check_docs.py`, a step of `make gate`, fails on a larger one of either, on `core/CLAUDE.md` over 8,000 bytes or `app/CLAUDE.md` over 16,000, and on any `core/MODULES.md` or `app/FILES.md` entry line over 200 characters.
- The roadmap is a live working doc, not a status doc; the cap and the check do not apply to it.

### README

- `README.md` is a short front page; the detailed feature reference is `public-docs/src/content/docs/reference/features.md`. A batch edits neither, except to fix a statement in one that the batch makes false; that fix goes in the same batch.
- A periodic sweep keeps both current. `python3 scripts/readme_sweep.py` lists the commits since the marker (README's last line, `<!-- readme-swept-to: <sha> -->`) with the `designs/` docs each changed. The sweep reads each commit's message (`git show -s <sha>`) and its status-doc entry; edits the feature reference wherever those commits changed what it says, and README only where its front-page summary changed; checks the generated MCP tool reference, `public-docs/src/content/docs/reference/mcp-tools.md`, against a running app's scratch socket or a built bundle's catalog (`python3 scripts/generate_mcp_reference.py --check --socket <DR_AGENT_SOCKET>` or `--check --catalog build/Build/Products/Debug/Doctored.app/Contents/Resources/agent-catalog.json`), and when it is stale runs the same command without `--check` to rewrite it; and moves the marker to the last commit it read, never to a placeholder for its own commit.
- The sweep runs before every release (a `publish.yml` run) and at least every 20 commits (`python3 scripts/readme_sweep.py --count`).

### Releases and the updater

- `publish.yml` archives `project.yml`'s third configuration, Distribution: Release plus the Sparkle updater (`DR_UPDATER`, the feed URL and the EdDSA public key, set there and nowhere else). Debug and Release are what runs locally. Distribution is never launched as built (an ad-hoc hardened build cannot load the ad-hoc framework, and `scripts/app_under_test.py` refuses its path); the one local launch is `scripts/updater_check.py` D9's scratch copy, re-signed without the runtime, under automation.
- Sparkle is named only inside `#if DR_UPDATER` in `app/Sources/AppUpdater.swift`: `make typecheck` and the probes have no Sparkle module, so `DR_UPDATER` never goes in a `ProbeSet.defines`. The framework is embedded in every configuration, and `-dead_strip_dylibs` keeps its load command out of Debug and Release (the gate's rows R1–R5 check Release; Debug compiles the same sources without `DR_UPDATER`). The updater never starts in a launch a script owns (`AutomationLaunch`), and waits for the first activation in an instance `doctored-mcp` opened (`-DRMCPHelperLaunch`).
- Every installed copy reads `https://github.com/kpgalligan/doctored/releases/latest/download/appcast.xml` and compares `CFBundleVersion`, the workflow's run number, so the repository, the asset names `Doctored.dmg` and `appcast.xml`, and the name of `publish.yml` never change; Latest is always the highest run and carries an appcast.
- The EdDSA private key (the `SPARKLE_ED_PRIVATE_KEY` secret; one key shared with Frame Job) is the owner's: no agent generates, reads, exports or prints it, or touches its Keychain item. `designs/sparkle-updates.md` holds the decisions and the owner's commands.

### FFI contract

`core/include/doctored_core.h` is a **hand-maintained** C header — the single contract between the worlds (imported via `app/Bridging/Doctored-Bridging-Header.h`). Changing any `dr_*` signature means editing the Rust `ffi_*.rs` file, the header, and the Swift wrapper (the area's `app/Sources/DoctoredCore+<Area>.swift`; `DoctoredCore.swift` holds the handle classes) in lockstep. Conventions, uniform across the surface:

- Opaque handles, each with its own `dr_*_free`: the byte path's `DrImage` (one image) and `DrDocument` (canvas + bottom-first layer stack), whose pixels are straight (non-premultiplied) RGBA8, row 0 = top; the typed `DrPrecisionDocument` and `DrPrecisionImage` for 16-bit and 32-bit float RGB documents and every colour-mode document (Grayscale, Duotone, Bitmap, Indexed, Lab, CMYK), 8-bit ones included; and the job-control, clipboard-payload, export-result, liquify, puppet and RAW-local-mask handles.
- **All operations are pure** — they return a new handle and never mutate the input, except the caller-owned buffers core/CLAUDE.md, "Purity", lists. Undo/redo on the Swift side is therefore just a stack of handles (`levelsOfUndo` is 0; retention is bounded by the per-document history byte budget — `set_history_budget`, Phase 5).
- Every export tolerates NULL inputs and never lets a panic cross the boundary (the Rust side's rules: core/CLAUDE.md, "FFI exports" and "Errors"); errors come back through an `err_out` heap CString. Strings the library returns (reports, JSON, `err_out` messages) are `CString::into_raw` allocations freed with `dr_string_free`. The one reverse case is a tool handler's reply, made with `dr_agent_string_create`, which the library reclaims; the host never frees it.
- Byte-path selections cross the boundary as canvas-sized u8 coverage masks (0 outside, 255 inside, intermediate = anti-aliased edge). Native selections and the precision-document APIs use u16 coverage (0…65535); `CanvasSelection.maskSamples16()` keeps those exact samples, and `maskBytes()` is the explicit byte adapter. Byte painting strokes use a canvas-sized **premultiplied** RGBA8 overlay handed to `dr_doc_painting_layer`; typed operations declare their separate native coverage inputs in the header.

### Where a feature lands — the standard recipe

Every shipped feature follows the same shape; putting a step's code anywhere else is what bloats files:

1. Pure model + ops in a **new Rust module**, tests in a new test file (core/CLAUDE.md, "Modules and size" and "Tests").
2. FFI three-file lockstep (above): `ffi_*.rs` shim → header → `DoctoredCore+<Area>.swift`.
3. Swift model/value types + `extension RasterDocument` / `RasterPrecisionDocument` in a **small dedicated file** (the `AdjustmentLayer.swift` pattern: value types plus one `extension RasterDocument`).
4. Canvas gesture handling as a session struct plus `on*` closures in `ImageCanvasView` — wiring only, logic in the feature's file.
5. Controller logic in an `EditorViewController+<Feature>.swift` extension file; menu items and their validation as app/CLAUDE.md, "Placement", says.
6. MCP parity: handler, facts and catalog entry (app/CLAUDE.md, "Agent parity"). A colour-mode document answers only the tools in its mode's closed `tools` set and the menu actions in its `selectors` set (`GrayDocument`, `LabDocument`, `IndexedDocument`, `BitmapDocument`, `CmykDocument`, whose tools are the `.cmykTool` facts; Duotone adds its own refusals before Gray's), so a feature a mode should offer is added to that mode's sets.
7. Dialogs as a `…SheetController` (app/CLAUDE.md, "Placement").
8. Verify as "Verification" below says. README and the feature reference are not edited per batch (see "README").

## Verification

- In the loop: targeted `make t F=<file> T=<filter>`, `make lint` after Rust edits, `make app-debug` after Swift or header edits, and the drives the change needs, against the Debug app.
- After an edit to a probe's Swift (`scripts/<probe dir>/*.swift`): `python3 scripts/probe_build.py --typecheck scripts/<probe dir>`. It stops before SILGen, like `make typecheck`; the probe's runner is what builds and runs it. The runners that build through `scripts/probe_build.py` keep the linked probe in `build/probe-cache`, so a second run on unchanged sources compiles nothing. `DR_PROBE_BUILD_ONLY=1` ends a runner that builds through `scripts/probe_build.py` right after the build, before it launches anything; that is how a `scripts/*_ui.py` drive's build is checked.
- Once, when the code is complete and before review: `make gate`. It is the only Full verification command; a spec lists it once, plus any Release-only drive the batch needs, plus `make dist` after a change to the updater surface (`project.yml`'s configurations or packages, `AppUpdater.swift`, `publish.yml`, `scripts/updater_check.py`, `scripts/appcast.py`, `Acknowledgements.txt`): run after `make gate` passes, never while it runs; it is the only local build that compiles the `DR_UPDATER` code.
- After a fix pass: `make gate` again. It re-runs only the legs whose inputs changed.
- Never run the whole suite, `make typecheck` before an app build, or a gate step by hand as a substitute; never run a check twice to see its output differently.
- When a refusal or error string changes, grep `core/tests` for the old wording before running anything.

`make gate` ends with the `probes` step: the same type check of each probe set whose directory under `scripts/` has uncommitted changes, and of each set whose last type check failed, until it passes. The step fails on a `scripts/` directory that holds Swift and is in neither `PROBE_SETS` nor `STANDALONE_DIRS` of `scripts/probe_build.py`.

Before it, one step per entry of `PROBE_RUNS` in `scripts/gate.py` runs a probe runner's small run under `DR_BACKGROUND=1`: `precision-probe`, `history-probe`, `selection-history-probe`, `selection-probe` (its `--synthetic` run), `save-preservation-probe`, `color-metadata-probe`, `data-migration-probe`, `path-guard-probe` and `updater-probe`. Each probe is compiled (under `DR_PROBE_BUILD_ONLY=1`) beside the Rust and Swift legs, at most two at a time, and run once every other leg has ended, because the verdicts include wall-clock bounds. A step is skipped while its inputs (the app sources, the header, `project.yml` and `VERSION`, the probe directory and the helpers its entry names, the core library) are unchanged since its last pass, and none runs a `--large*` mode. A runner joins as one entry: it takes `DR_PROBE_BUILD_ONLY` itself (`probe_build.take_build_only`), never comes to the front under `DR_BACKGROUND=1`, measures no time or memory, and reads nothing outside the tree.

`make gate` takes `GATE_ARGS="--force"` to run every step (the probe-run steps included); the probes step still covers only probe directories that changed or failed last time, so a committed probe directory is checked with `python3 scripts/probe_build.py --typecheck scripts/<probe dir>`. It returns non-zero for any outcome but a pass, and its last line says which: `gate: FAIL (<steps>)` names the failing steps (`(gate)` means the script itself raised, with the traceback on stderr); `gate: BLOCKED ...` means another build, test run or gate in this checkout was running and nothing ran, so run it again when that one ends. Logs are `build/gate-logs/<step>.log`.

### Parallel worktrees

- Batches run in parallel lanes: the main checkout and git worktrees under `.claude/worktrees/lane-*`. Each lane builds its own `core/target` and `build/`.
- `make gate` holds its own checkout's `build/gate.lock` and waits only for builds, test runs and gates in that checkout, never another lane's.
- Wall-clock measurements (`make cancel-bench`, the time-measuring Release-only drives) need the machine idle: run them only when no other lane is building.

## Driving a built app for verification

The MCP server is the reliable way to exercise the app end-to-end (open documents, run tools, render the canvas as PNG, check undo). It listens only on a Unix socket (`designs/agent-socket.md`): `~/Library/Application Support/Doctored/Agent/agent.sock`, which no test may bind, or the path `DR_AGENT_SOCKET` names. Every test launch sets `DR_AGENT_SOCKET` to a scratch socket of its own (a fresh `0700` folder under `$TMPDIR`, for macOS's 103-byte socket path) and is a child of the script that started it:

```python
import os, subprocess, tempfile
from pathlib import Path
import app_under_test                                         # scripts/app_under_test.py
from mcp_client import Client, remove_scratch, scratch_socket, wait_ready   # scripts/mcp_client.py
from verification_isolation import preset_environment         # scripts/verification_isolation.py
exe = app_under_test.default_app_executable()   # the Debug app's absolute path unless DR_VERIFY_APP names another
app_under_test.require_fresh_app(exe)
folder, socket = Path(tempfile.mkdtemp()), scratch_socket()
app = subprocess.Popen(
    [str(exe), "-ApplePersistenceIgnoreState", "YES"],
    env=dict(os.environ, DR_BACKGROUND="1", DR_AGENT_SOCKET=str(socket), ANTHROPIC_API_KEY="dummy",
             DR_WORKING_LIMIT_BYTES="2147483648", DR_AUTOMATION_ALERTS="record", DR_EDR_HEADROOM="1",
             DR_ACTIONS_DIR=str(folder / "actions"), DR_RECOVERY_DIR=str(folder / "recovery"),
             **preset_environment(folder)))
client = Client(socket); wait_ready(client, app, 30, "app log")
# client.call("list_documents") ...; then app.terminate() and remove_scratch(socket) in a finally
```

`preset_environment` points every private directory the app reads besides Actions and recovery (`PRESET_DIRECTORIES` in `scripts/verification_isolation.py`; app/CLAUDE.md, "Verification isolation", has the rest of the isolation rules) into the private folder and every cloud provider at a dead loopback port (`PROVIDER_BASE_URLS`); `DR_EDR_HEADROOM=1` keeps HDR Preview from following the screen the instance lands on. `verification_isolation.owned_environment(folder, socket)` returns the same additions but `DR_EDR_HEADROOM` (the gate's `launch` and `updater_check.py` D9 use it). `scripts/mcp_client.py` is the one client every drive and launcher uses (HTTP over the socket, chunked bodies included); `phase5_support.App`, `verify_phase1.instance` and the drives' own `App` classes do the same, each with its own isolation folder, except that the shared launchers (listed under `DR_BACKGROUND` below) take `DR_BACKGROUND` from the caller's environment (`export DR_BACKGROUND=1`). `curl --unix-socket <socket> -H 'Content-Type: application/json' --data-binary @- http://localhost/mcp` reaches it from a shell. External clients use the bundled stdio helper `Contents/Helpers/doctored-mcp` (`mcp-proxy/`; Settings ▸ MCP copies its `claude mcp add` line); `scripts/verify_mcp_helper.py` drives it against an owned instance.

Owned test instances pin the working limit with `DR_WORKING_LIMIT_BYTES=2147483648`, so admission and refusals match the 2 GiB reference limit on every machine instead of following its RAM.

Always launch test instances with a dummy `ANTHROPIC_API_KEY`: the variable wins over the stored key and the legacy keychain copy, so a test instance never reads the user's key or blocks launch on the one-time keychain migration prompt.

`DR_BACKGROUND=1` makes the instance an `.accessory` app (no Dock icon, no menu bar) that never activates itself and orders its windows behind other apps' windows while it is inactive — set it for every test launch (`app/Sources/BackgroundLaunch.swift`). `export DR_BACKGROUND=1` once reaches every `scripts/` launcher with no edits, because all of them build the child env from `dict(os.environ, …)` / `os.environ.copy()` (the shared launchers are `phase5_support.App`, its `host-source-probe/support.py` `HostApp` subclass, `verify_phase1.instance` and `verify_phase9i_assisted.instance`; drives with their own `App` class, such as `verify_v1_lens.py`, build their environment the same way). The `scripts/*_ui.py` drives compile their own probe `main.swift` and activate themselves on purpose (their synthesized key events need a key window), so they still come to the front with the variable set; setting it does not change their outcome. A background instance is not active, so an MCP call that omits `document_id` cannot rely on a main window — pass `document_id` when more than one document is open.

`scripts/` drives default to the Debug app through `scripts/app_under_test.py`; `DR_VERIFY_APP` selects another app; the launchers refuse a binary older than the Swift sources, the header, the core library, the `doctored-mcp` sources or the `doctored-agent` package and name the `make` target; `DR_ALLOW_STALE_APP=1` overrides. They refuse a Distribution build (`make dist`'s product) whatever that variable says.

Owned instances never start the updater (`AutomationLaunch`), and Debug and Release have none.

Automated runs never show a modal alert or error (`app/Sources/AutomationAlerts.swift`): under `DR_BACKGROUND=1`, or `DR_AUTOMATION_ALERTS=record` (which every `scripts/` probe runner and launcher sets), each `NSAlert` is answered with its first button and each error presentation as "not recovered", and closing a dirty document is cancelled, never saved or discarded. Each is logged as one `[AutomationAlerts]` stderr line, so send a drive's stderr to a file and grep it. File panels and `ModalCardWindowController` dialogs are not suppressed.

Use a scratch `DR_AGENT_SOCKET` and argv-style defaults so nothing leaks into the user's own instances. The user often has their own Xcode-launched Doctored running — never signal or kill Doctored processes you didn't launch; track your own by the `Popen` handle you hold, never by a process-name search. Prefer opening documents via the `open_document` MCP tool over launch-time argv paths.

### Release-only drives

These measure time or memory, so they are not run against the Debug app.

- `scripts/verify_phase4.py` — measures time; runs the Release app (`DR_VERIFY_APP=build/Build/Products/Release/Doctored.app` after `make app`).
- `scripts/phase4_raw_latency.py` — measures time; compiles its own probe against the release core (`make rust` first) and launches no app.
- `scripts/raw-decode-probe/measure.py` — measures memory (phys_footprint); builds its own `-O` probe against the release core and launches no app.
- The admission drives (`scripts/*-admission/measure.py`, `scripts/host-still-admission/verify.py`) — measure memory on the Release app after `make app`; each names it with `--app` or `DR_VERIFY_APP`, as its header or README says.
