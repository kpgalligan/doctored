# Build velocity: shipped phases

The settled record of the shipped phases of `designs/build-velocity-plan.md`; the open phase and its working record are in [build-velocity-status.md](build-velocity-status.md).

## Phase 1

Shipped in `8384ac9` (2026-10-06): a batched `make typecheck`, and `make app-debug`, the Debug app the edit loop and the drives use. "Before" is the proposal's number for the command the loop used until then.

| Measurement | Command | Before | After |
|---|---|---|---|
| Type check, clean tree | `make typecheck` | 203 s | 12.6 s |
| Type check, one injected type error | `make typecheck` | 145 s | 12.6 s, exit 2 |
| App build, cold, with a 23.1 s core rebuild | `make app-debug` | 171 s (`make app`, Release) | 62.1 s |
| App build, no change | `make app-debug` | 1.3 s (`make app`) | 1.8 s |
| App build, one statement changed in `app/Sources/CropTool.swift` | `make app-debug` | 156 s (`make app`) | 14.3 s |

- `DEBUG_XCFLAGS := COMPILER_INDEX_STORE_ENABLE=NO ENABLE_DEBUG_DYLIB=NO`: both are kept as "not slower", not as a measured gain. With the second the product has no `Rasterize.debug.dylib`; `Contents/MacOS/Rasterize` is the whole binary. The first build after a settings change recompiles every file.
- Cargo runs on every app build, once from `make` and once from Xcode's script, and rebuilds nothing when the core is unchanged. So a bare `xcodebuild` after a Rust edit rebuilds the core and relinks the app.
- The `scripts/` launchers refuse an app older than the Swift sources, the header or the core library, and name the `make` target. `RZ_ALLOW_STALE_APP=1` overrides; `RZ_VERIFY_APP` selects another app.

## Phase 2

Shipped in `6630ce3` (2026-10-06). `core/CLAUDE.md` and `app/CLAUDE.md` keep their rules; every per-file entry moved to `core/MODULES.md` (161 entries) and `app/FILES.md` (130), which are grepped by file name and never read whole. The root `CLAUDE.md` imports the two rule files.

| Measurement | Command | Before | After | Limit |
|---|---|---|---|---|
| `core/CLAUDE.md`, bytes | `wc -c core/CLAUDE.md` | 282663 | 4856 | under 8000 |
| `app/CLAUDE.md`, bytes | `wc -c app/CLAUDE.md` | 222189 | 14809 | under 16000 |
| `core/MODULES.md`, bytes | `wc -c core/MODULES.md` | no file | 278278 | — |
| `app/FILES.md`, bytes | `wc -c app/FILES.md` | no file | 207988 | — |

- The move is verbatim: `python3 scripts/verify_claude_md_split.py --base 8384ac9` exits 0. No entry was shortened, so 156 core and 128 app entries are longer than the 200 characters a new entry may have. `scripts/check_docs_index_baseline.txt` lists them until Phase 9c shortens them.
- The import form of the root file shipped: a subagent's first turn holds the root file and both rule files, and neither index.
- Nine Swift comments that cited `app/CLAUDE.md` for a moved entry now name `app/FILES.md`. Dated slice records and everything under `archive/` are not rewritten.

## Phase 3

Shipped in `c46f334` (2026-10-06). Each of the 100 in-scope `scripts/` drives ran against the Debug app, then the Release app, serially, under a 1200 s limit per run, both apps built from `6630ce3`. Drives with a host half ran their app half only, and nothing launched Photoshop, GIMP or a network fetch.

| Measurement | Command | Debug | Release |
|---|---|---|---|
| The 77 drives that pass on both, in total | `python3 scripts/<drive> <arguments>`, one run per app | 894 s | 816 s |
| Largest ratio | `python3 scripts/phase6_initial_masks.py` | 6 s | 2 s |
| Largest difference | `python3 scripts/verify_phase12b_actions.py` | 47 s | 31 s |
| The one Release-only drive | `python3 scripts/verify_phase12g_clipboard.py` | timeout, 1216 s | pass, 1114 s |

- **Outcomes:** 77 `ok`, 1 `Release-only`, 11 `fails on both`, 11 `NOT RUN`. The per-drive table is in the archived original.
- **The drives default to the Debug app.** No Debug run trapped, so the 58 assert sites stand. A drive counts as slow when Debug is more than 3 × Release and more than 60 s longer; none is.
- **Release-only:** `verify_phase12g_clipboard.py`. On Debug the MCP document-id lookup rescans every open document; it is long on both apps because every background wait builds a progress panel. The list is in the root `CLAUDE.md`.
- **Fails on both** (the same check on both apps; none fixed here): 8B and 8C, a frozen tool-catalog count; 8A and 11E, `WRAPPER_WORDS`; two 10D drives, a reworded Pass Through refusal; 11D gate D6, no JPEG 2000 reader in ImageMagick; 12A gate r9; 12F-1 gate b5; 9B's 16-filter stack; Phase 4's 48 MP corpus.
- **Not run:** eleven drives have no inputs on this machine.
- `verify_phase12e3c_psd.py --skip-photoshop` ends `INCOMPLETE` with exit 2, which counts as that invocation's pass. `verify_phase12f3_warp.py` rewrites a tracked docs page.

## Phase 4

Shipped in `4a3c70e` (2026-10-06): `make gate` (`scripts/gate.py`), the parallel test runner (`scripts/run_tests.py`) and the Verification rule. 408 test binaries, 655 Swift files.

| Measurement | Command | Result | Target |
|---|---|---|---|
| First run, every test binary rebuilt | `make gate` | `gate: PASS 457.6s` (tests 430.7) | under 300 s; cap 600 s |
| No change | `make gate` | `gate: PASS 0.7s`, every leg `skip` | every leg skipped |
| Final run: both legs, no test binary rebuilt | `make gate` | `gate: PASS 228.5s` (tests 216.7, app 169.0) | under 300 s |
| Whole suite, three runs | `python3 scripts/run_tests.py` | 215.0 s, 213.0 s, 214.2 s | about 220 s |
| Whole suite, serial | `make test-serial` | 361 s | about 500 s |

- **Target.** Every run was under the 600 s cap. The 300 s target is met only when no test binary is relinked: relinked binaries run about 2.5× slower the first time. Phase 5's merged test binaries brought the gate after a `core/src` edit from 413.3 s to 299.3 s, under the target by 0.7 s in one run (see Phase 5).
- **Outcomes.** `make gate` returns status 2 for every outcome but a pass, and its last line names it (`gate: FAIL (<steps>)`, `gate: BLOCKED …`).
- **Hash inputs.** Rust: `core/`, `scripts/run_tests.py`, `scripts/gate.py`. Swift: `app/`, `agent/`, `core/include/rasterize_core.h`, `project.yml`, `Makefile`, `VERSION`, `scripts/gate.py`, `scripts/app_under_test.py`, `scripts/verification_isolation.py`. `.md` files are left out of both.
- **`core-build` runs on every run** that gets past `wait`, so the hashed library is the one the app links.
- **Solo list.** The plan's eight, none added; they take 150 s of each run.
- **"png" assertion (plan item 4).** No code change: all eight sites already assert on the message with the path removed.

## Phase 5

Done 2026-10-07: the 407 one-file test binaries are 8 suite binaries (`core/tests/suite_*.rs`, which mount 323 files by `#[path]`) and 84 standalone ones, with no test file edited or moved. `core/Cargo.toml` sets `autotests = false` and lists every target; `core/tests/test_layout.rs` fails on a test file that nothing builds, on one built twice and on a standalone entry with no reason. `make t F=<file> T=<filter>` (`scripts/run_test_file.py`) runs one file from whichever binary holds it and fails when no test runs. `scripts/verify_test_merge.py` lists the same 3,932 tests before and after, 44 of them ignored; the three guard tests are the only additions.

| Measurement | Command | Before | After |
|---|---|---|---|
| Test executables, with the library's unit-test binary | `python3 scripts/run_tests.py`, its last line | 408 | 93 |
| Test build after a library edit, nothing else running | `cd core && cargo test --release --no-run` after a `#[used]` static changes `core/src/durable_file.rs` | 46 s | 23 s |
| Whole suite, three runs | `python3 scripts/run_tests.py` | 215.0 s, 213.0 s, 214.2 s | 234.8 s, 234.1 s, 233.8 s |
| Gate after a `core/src` edit | `make gate` after the same edit | `gate: PASS 413.3s` (tests 383.0, 75 s of it the rebuild) | `gate: PASS 299.3s` (tests 274.2, 24 s of it the rebuild) |
| `core/target/release` after a clean build | `du -sk core/target/release` after `cargo clean --release` and a build of every test binary | 9,490,176 KB | 4,943,768 KB |

- **Standalone binaries (84).** Each `[[test]]` entry gives its reason after `# isolate:`. Own allocator: 8. Process state (the working limit, the working directory): 2. The process-wide view cache and memos: 31. A wall-clock assertion in a default test: 29. A deadline-bounded poll in a default test: 8. One large document at a time: 6.
- **Moves.** None. The first three whole-suite runs passed with `ok: 93 binaries`, so no file left a suite and `SOLO` keeps its eight names.
- **The whole suite is about 20 s slower.** The eight suites run one at a time and take 67 s, 29 s of it `suite_regional`; the 77 pooled binaries take about 18 s; the solo list is unchanged at 149 s. Before, everything outside the solo list shared the pool and took about 65 s.
- **The 300 s gate.** Resolved by its own test, with no margin: the gate after a `core/src` edit took 299.3 s against the 300 s target, in one run. Its test run took 250 s on relinked binaries against 234 s on unchanged ones (308 s against 215 s in Phase 4). The gate with every test binary rebuilt after `cargo clean --release` is `gate: PASS 499.9s` (tests 445.7, 190 s of it the build).
- **`make cancel-bench`** runs each of its 18 files through `scripts/run_test_file.py`: the same 29 ignored tests, 79 s.
- **Open.** Comments and `#[ignore]` reasons in 16 test files still spell `cargo test --test <file>`, which no longer names a target for the 15 that are mounted.

## Phase 6

Phase 6a done 2026-10-07: `scripts/probe_build.py` builds an app-hosted probe once per source state, the gate has a `probes` step, and seven of the plan's 20 runners build through the helper (`history-probe`, `selection-history-probe`, `jobs-probe`, `selection-probe`, `save-preservation-probe`, `precision-probe`, `color-metadata-probe`, each `scripts/<probe>/verify.py`). Phase 6b converted the other 13, below. "Before" is the same runner with its inline `swiftc` call, measured before any edit.

| Measurement | Command | Before | After |
|---|---|---|---|
| Probe run, sources unchanged since the last run | `python3 -B scripts/history-probe/verify.py --keep`, second run | 66.6 s | 4.6 s, `reused build` |
| Probe run, nothing cached | the same, after `rm -rf build/probe-cache build/probe-module-cache` | 67.4 s | 59.9 s |
| Probe run after one comment line is appended to `scripts/history-probe/main.swift` | the same | 66.6 s (the unedited run; every run compiled) | 55.3 s |
| The seven converted runners, one run each, every one compiling | `python3 -B scripts/<probe>/verify.py --keep` | 832.7 s | 746.6 s |
| Type check of the app sources with one probe directory | `python3 scripts/probe_build.py --typecheck scripts/history-probe` | no such check | 15.3 s |
| The gate's `probes` step on seven changed probe directories | `make gate` | no step | 87.7 s of `gate: PASS 341.4s` |

- **The gain is the reuse, not the compile.** A run on unchanged sources copies the linked probe and compiles nothing. A run that compiles is 7 to 18 s shorter; the build adds `-num-threads` (threaded code generation) and a module cache that persists, and which of the two the seconds come from was not measured.
- **The cache.** `build/probe-cache/<key>/` holds `probe`, `manifest.json` and `build.log`; the eight entries used last are kept (376 MB for eight, `du -sh build/probe-cache`). The key covers the repo's absolute path, the helper, the compiler version, the SDK, the command line, every source, the bridging header and what it includes, the archive, the Info.plist and any extra object. A file inside the repo is keyed by its path and a file outside by its name, so an in-place build never shares an entry with a snapshot build (`jobs-probe` finds the repo through `#filePath`). `build/probe-module-cache` is the module cache of every build and type check.
- **Same probe.** For each of the seven the new `build.log` command line differs from the old one only by the `xcrun` prefix, `-num-threads 12`, `-module-cache-path` and the `-o` path, and the run has the same exit status and the same `PASS` and `FAIL` lines.
- **The `probes` step** type-checks each probe set whose directory under `scripts/` has uncommitted changes, and each set whose last type check failed (stored as `"probe:<name>": "failed"` in `build/gate-state.json`) until it passes. `--force` re-checks those sets and adds none. It fails on a `scripts/` directory that holds Swift and is in neither `PROBE_SETS` nor `STANDALONE_DIRS`; that check covers every directory on every run. With `let x: Int = ""` appended to `scripts/native-paint-probe/main.swift` the gate ended `gate: FAIL (probes) 14.5s`; after `git checkout` the next ended `gate: PASS 12.9s` with `probes  exit=0`, and the one after `gate: PASS 0.5s` with `probes  exit=skip`.
- **The type check stops before SILGen**, like `make typecheck`. Only the runner's build proves the probe links.
- **Open.** `color-metadata-probe` exits 1 before and after, on `FAIL [metadata limits]: large XMP format limit jpg`. A committed probe directory is not type-checked by the gate; all 20 sets passed the type check at `fa47689`. `scripts/test_provenance_manifests.py` fails at `HEAD`, before and after 6b alike: 54 manifests miss `scripts/app_under_test.py`. `verify_phase8c.py --host-only`'s host probe hangs until its 600 s timeout, before and after 6b. The 8G drive has no corpus here, so its probe was only built.

Phase 6b done 2026-10-07: the 13 remaining runners build through `scripts/probe_build.py`: the five `scripts/verify_phase9{d,e,f,g,h}_ui.py` drives, `scripts/artwork-text-probe/run_host.py` (both sets), the `host()` of `verify_phase8c.py`, `8d`, `8f`, `8g` and `9a`, `scripts/phase4_raw_latency.py` and `scripts/raw-decode-probe/measure.py` (`-O`). So all 20 of the plan's runners share the cache. `RZ_PROBE_BUILD_ONLY=1` ends a runner right after its build, before it launches anything; that is how the UI drives' builds were checked without running them. "Before" is the same runner with its inline `swiftc` call, measured before any edit.

| Measurement | Command | Before | After |
|---|---|---|---|
| 8C host run, first then second run | `python3 -B scripts/verify_phase8c.py --host-only --keep` | 671.3 s, exit 1 | 648.9 s (`built`), then 600.8 s (`reused build`, same key); exit 1 both, the probe's 600 s timeout |
| 9D UI drive's build, first then second run | `RZ_PROBE_BUILD_ONLY=1 python3 -B scripts/verify_phase9d_ui.py --report … --fixtures …` | no such switch | 50.9 s (`built`), then 0.67 s (`reused build`, same key) |
| Type check of all 20 probe sets at `fa47689` | `python3 scripts/probe_build.py --typecheck scripts/<probe>` | — | 20 of 20 pass, 11.6 to 14.7 s each |

- **Same probe.** For all 14 invocations (`run_host.py` twice) the new `build.log` command line differs from the old one only by the `xcrun` prefix, `-num-threads`, `-module-cache-path` and the `-o` path. The full runs (`run_host.py`, 8C, 8D, 8F, 9A, `phase4_raw_latency.py`) have their baseline's exit status and pass lines.
- **No `-num-threads` when basenames clash by case.** Threaded WMO names one object per source basename; on a case-insensitive disk `scripts/native-selection-probe/selection.swift` and `app/Sources/Selection.swift` overwrite each other's object and the link fails (likewise three files of `native-paint-probe`). The helper builds such a set on one thread and prints the clashing names; 9G and 9H then match their old command lines exactly.

## Phase 7a

Phase 7a done 2026-10-07: `RasterCore.swift` keeps the handle classes (`RasterPrecisionDocument`, `RasterImage`, `RasterDocument`), the error plumbing, `Int.clamped`, `RasterCoreProcess` and `RasterJobControl`; its other sections moved verbatim into 44 `RasterCore+<Area>.swift` files, a class-body section inside `extension <Class> { … }`. `AgentCatalog.swift` keeps `catalogJSON`, `tool`, the shared `CatalogSchema` and their helpers; its 14 `@inline(never)` area builders moved verbatim, each into an `AgentCatalog+<Area>.swift` inside `extension AgentServer { … }`. No rename, no caller edit, no call-order change. Both hubs join the frozen list (`app/CLAUDE.md`, `app/FILES.md`).

| Measurement | Before (`ad33e84`) | After |
|---|---|---|
| `wc -l app/Sources/RasterCore.swift` | 7,889 | 1,178 |
| `wc -l app/Sources/AgentCatalog.swift` | 3,999 | 536 |
| `ls app/Sources/*.swift \| wc -l` | 655 | 713 (44 `RasterCore+`, 14 `AgentCatalog+`) |
| `tools/list` from the Debug app over MCP | 436,786 bytes, 269 tools; two launches `cmp`-identical | `cmp`-identical to the first launch before |

- **Widened access.** 36 declarations lost `private`/`fileprivate` because their callers now sit in another file, each one the compiler reported: the top-level `precisionJSON`, `precisionCString`, `takeErrorMessage` and `rawLocalContext`; 15 members of the handle classes and `RasterRawLocalMask` (`wrap`, `isValidIndex`, `fallible`, `withMask`, `evictMixedCaches`, …); in `AgentCatalog.swift` `adjustmentOps`, `CatalogSchema`, `layerList` and the 14 moved builders. No other app file declares any of these names.
- **The split was checked by script.** A script built the files from `git show ad33e84` and a second checker confirmed it: each hub's map covers every original line once, and every destination equals its imports plus its chunks once the widenings and the one comment edit are applied (`check_split: ok (60 files)`). `make typecheck` had 0 errors and `make gate` ended `gate: PASS 221.6s`.
- **`rz_*` calls.** Outside the `RasterCore*.swift` files, only the four server-lifecycle calls in `AgentServer.swift` remain (`rz_agent_server_start`, `rz_string_free`, `rz_agent_server_stop`, `rz_agent_string_create`).

## Phase 7b

Phase 7b part 1 done 2026-10-07: the 269-entry `AgentServer.handlers` literal became one `static let <feature>Handlers` table per file that declares the handler methods (73 `AgentServer+<Feature>.swift` files, `ArtworkRasterization.swift`, `PrecisionModeConversion.swift`, `ScalarMorphology.swift`, and `coreHandlers` in `AgentServer.swift` for its 30 private handlers). `AgentServer+Registry.swift` lists the 77 tables in `handlerTables` and merges them into `handlers`, whose name and type callers already used. Each entry was copied unchanged; no handler body, signature or visibility changed.

| Measurement | Before (`6e5d2e6`) | After |
|---|---|---|
| `wc -l app/Sources/AgentServer.swift` | 2,587 | 2,266 |
| Handler tables | 1 literal | 77 (`check_handlers: ok (77 tables, 269 tools)`) |
| `tools/list` from the Debug app over MCP | 436,786 bytes, 269 tools; two launches `cmp`-identical | `cmp`-identical to the first launch before |

- **The parity check runs in Debug and Release.** At server start, a catalog/dispatch mismatch, or a tool declared in more than one table, writes one `[AgentServer] catalog parity failure: …` line to stderr and exits with status 1. It was shown on scratch edits that were not kept. A `parity_probe` entry gave `… in dispatch but not catalog: ["parity_probe"]` and `exited 1` in Debug and in Release, with no `tools/list` reply. A second `get_view` entry in `viewsHandlers` gave `… declared in more than one table: get_view (viewsHandlers, workspaceHandlers)` and `exited 1`.
- **The `tools/call` replies are equal, but not always byte-identical.** The replies for `list_documents` and `no_such_tool` match in content before and after. `list_documents`'s inner JSON changes key order from launch to launch, because `jsonResult` serializes a Swift dictionary whose hash seed is per process. One after-capture in five had the other order. That variation was there before this change.
- **`make gate`** ended `gate: PASS 159.3s`; its launch leg started the Release app with the server on, so the parity check ran there.
- **Left for Phase 7b-2:** the menus in `AppDelegate.swift` (per-menu builders, `buildMainMenu` as the ordered registry), the `ActionCatalogFacts` sets, and the `DocumentPrecision` whitelists with `FloatDocument.tools`. Each gets a `#if DEBUG` dump captured before any move.

Phase 7b part 2 done 2026-10-07: the 30 menu builders in `AppDelegate.swift` moved verbatim into 11 `AppDelegate+<Menu>Menu.swift` files, one per top-level menu, and `buildMainMenu()` stays their one ordered list. The only edit to a builder is dropping `private` from the 11 top-level ones, which `buildMainMenu()` calls across files; sub-builders stay `private` beside their callers. Before the move, `RegistryDump.swift` added a Debug-only dump: with `RZ_REGISTRY_DUMP=<path>` the app writes the installed menu tree and nine tool-name sets to that file in `applicationWillFinishLaunching` and exits 0. Each capture ran in its own folders for all 12 `RZ_*_DIR` variables.

| Measurement | Before (`b7cfc7f` + dump) | After |
|---|---|---|
| `wc -l app/Sources/AppDelegate.swift` | 1,324 (before the hook) | 331 |
| Menu builder files | 1 | 11 (`check_menus: ok (11 files, 30 builders)`) |
| Dump, plain launch | 71,539 bytes, 398 items; two launches `cmp`-identical | `cmp`-identical to the first launch before |
| Dump, seeded shortcut set | differs from plain only in Duplicate Layer (⌃⌘J), Quick Mask Mode (⌥⌘9) and Rulers (no key), all tag 0 | `cmp`-identical to before |
| Dump, assisted annotation on | differs from plain only by the added File ▸ Open Assisted Annotation Case… | `cmp`-identical to before |
| Set counts in the dump | `ActionCatalogFacts`: repeatableTools 4, targetedRepeatableTools 4, nonRecordable 140, selectionSensitive 25; `DocumentPrecision`: commonTools 141, documentFreeTools 48, typedTools 74, rasterTools 20; `FloatDocument.tools` 133 | unchanged (no set moved) |

- **`make gate`** ended `gate: PASS 174.3s`. `grep -a -c RZ_REGISTRY_DUMP` on the Release binary printed `0`, so the dump is compiled out of Release.
- **Left for Phase 7b-3:** per-area tables for the four `ActionCatalogFacts` sets, the four `DocumentPrecision` whitelists and `FloatDocument.tools`. Each merged set keeps its name and members; this run's dump sections, compared as sets, are the baseline.

Phase 7b part 3 done 2026-10-07: the nine tool-name sets keep their names and types but are built from per-file tables. Each file that declares a `<prefix>Handlers` table now declares a `<prefix>ToolFacts` table beside it, with one `ToolFacts` option set per tool that is in any set (`[:]` when none is). `coreToolFacts` lives in the new `AgentServer+CoreToolFacts.swift`, because `AgentServer.swift` is frozen. `AgentServer+Registry.swift` lists the tables in `factTables`, index-paired with `handlerTables`, merges them into `toolFacts`, and each set is `AgentServer.tools(with: .<flag>)`. Every `//` block of the old literals moved verbatim above the entry of a tool it explains; a block whose subject spans files was copied into each.

| Measurement | Before (`5837a08`) | After |
|---|---|---|
| `wc -l` `ActionSteps.swift` / `DocumentPrecision.swift` / `FloatDocument.swift` | 1,700 / 682 / 471 | 1,575 / 500 / 326 |
| Facts | 8 set literals | 77 tables, 267 entries (`check_facts: ok (77 tables, 267 tools, 256 comment lines)`) |
| Dump, plain launch (menus and the nine sorted sets) | `cmp`-identical to part 2's after | `cmp`-identical to before |
| `tools/list` from the Debug app over MCP | 436,786 bytes | `cmp`-identical to before |

- **The start check covers the facts tables.** `enforceCatalogParity` now also reports a `factTables` entry that does not pair with `handlerTables`, a facts entry outside its paired handler table, a tool with facts in two tables, and two set rules: `documentFree` never with `commonTool`, `typedTool` or `floatTool`, and `rasterTool` only with `commonTool` or `typedTool`. Both rules held at `5837a08`. Not checked: `commonTools` and `typedTools` are disjoint (true, undocumented), and `FloatDocument.tools` is not inside `commonTools ∪ typedTools` (`get_tone_map`, `set_tone_map`). A scratch edit that was not kept dropped the last `factTables` entry, added `parity_probe` and `get_view` to `viewsToolFacts` and `.documentFree` to `list_views`. The Debug app gave no reply and `exited 1` with the one line `[AgentServer] catalog parity failure: tool facts tables do not pair with handlerTables: position 76: expected scalarMorphologyToolFacts, found none; tool facts outside their handler's table: get_view (viewsToolFacts), parity_probe (viewsToolFacts); tool facts declared in more than one table: get_view (viewsToolFacts, workspaceToolFacts); tool facts break a set rule: list_views (documentFree with commonTool, typedTool or floatTool)`.
- **`make gate`** ended `gate: PASS 174.7s`; its launch leg started the Release app with the server on, so the facts check ran there (`tools/list` answered with 269 tools).

Phase 7b is done.

## Phase 8

Phase 8 done 2026-10-07 (items 1–5): `image` takes explicit features, image 0.25's `default` minus `avif`; the library has no unit tests (`[lib] test = false`); the `[profile.release]` comment says which builds use it and why there is no LTO; and core/CLAUDE.md gains a **Layers.** rule that `scripts/check_layers.py` enforces under `make lint` for 55 kernel modules (every candidate passed; none dropped).

| Measurement | Before (`814da95`) | After |
|---|---|---|
| Cold build, `cd core && CARGO_TARGET_DIR=<scratch> cargo build --release --offline --timings`, serial | 51.2 s | 43.4 s |
| Units compiled (`cargo-timing.html` "Total units") | 150 | 105 (37 crates fewer, `ravif`/`rav1e` among them) |
| Lock packages, `grep -c '^name = ' core/Cargo.lock` | 139 | 139 |
| `image` features, `cargo tree -e features -i image --offline` | `default` (`rayon` + 15 formats, `avif` included) | `rayon` + 14 formats; `cargo tree -i ravif` / `-i rav1e` print nothing |

- **AVIF proof.** `rg -i avif` over `core/src`, `core/include`, `app/Sources` finds core comments and host (ImageIO) types only; the core's codecs are png, jpeg, bmp, gif, webp, and `rz_image::Format` has no AVIF arm. The `avif` feature is only `ravif`'s encoder. The lock keeps `ravif` and `rav1e`: image's weak features `ravif?/threading` and `ravif?/asm` keep them resolved, though nothing builds them.
- **Moved tests.** `cargo test --release -- --list` lists `doc_comps::tests::the_payload_cap_refuses_save_and_update_by_name` and `rzdc::capacity::warp_capacity_tests::serialized_fallback_planes_count_at_the_decoded_cap` once each before, and `layer_comps::…` and `persisted_warp_native::…` once each after. The seams are `#[doc(hidden)] pub` (`edit_layer_comps_capped`, `rasterize_core::warp_decoded_capacity`). `test_layout`'s new `no_library_source_holds_a_unit_test` fails on a unit test under `src/`.
- **Lint demo.** `type _LayeringProbe = crate::doc::RzDocument;` appended to `blend.rs` made `make lint` exit 2 with `error: core/src/blend.rs:665: names RzDocument; …`; `fn _p<'a>(_: &'a crate::doc::RzDocument, _: &'a u8) {}` made `check_layers.py` exit 1 on the same line. `blend.rs` ends unchanged (`git diff --quiet`).
- **Gate.** The `clippy` step runs `make lint`, so it reaches `check_layers.py`; `RUST_INPUTS` gains `Makefile` and `scripts/check_layers.py`, so editing either reruns the Rust leg. The lib unittest binary is gone: the tests step ran 92 binaries (93 before).
- **Decoders.** Which decoders to keep is the owner's call. No decoder was removed.
- **`make gate`** ended `gate: PASS 295.8s`.

## Phase 9a

Phase 9a done 2026-10-07: README is brought up to date by a sweep, not per batch. The rule is the root `CLAUDE.md` "README" section (recipe step 8 points to it): a batch edits `README.md` only to fix a statement it makes false. README's last line is the marker `<!-- readme-swept-to: <sha> -->`; `scripts/readme_sweep.py` lists the commits after it with the `designs/` docs each changed (`--count` for the number, `--since REV` for another base). The sweep runs before a push to `publish` and at least every 20 commits.

- **First sweep.** Read `fe895fd..5cad1a7` (17 commits); the marker names `5cad1a7`, and the sweep's own commit follows it. README changes: Building (each target's comment matches the root Commands block; `make lint`, `make project`, the probe type check and the sweep script added; a paragraph on `core/tests/`, the Debug app drives, probe builds, the registry dump and the rule files), Layout (`core/tests/`, the two indexes, `agent/`, `scripts/`, `designs/`, `archive/`, `public-docs/`), the MCP section's source-file sentence (catalog and handler-table files), and the assistant's tool figure (~250 to ~270).
- **MCP check.** Against the Release app, `generate_mcp_reference.py --check` reported the page stale (the Copy and Paste arguments, before `fe895fd`); it was regenerated with the script and the check then printed current. tools/list has 269 tools.
- **Issues.** None filed.
- **`make gate`** ended `gate: PASS 1.3s`: docs and core-build ran; every other step skipped, its inputs unchanged.

| Measurement | Before (`5cad1a7`) | After |
|---|---|---|
| `wc -c README.md` | 222400 | 224172 |
| `python3 scripts/readme_sweep.py --since fe895fd --count` | 17 | 17 |

## Phase 9c

One-line index entries (plan step 9c), in two runs.

- **Part 1** (done): every `core/MODULES.md` entry cut to at most 200 characters (161 entries, 175 lines), its detail moved into the named module's `//!` header (296 files, comment lines only); the 155 core lines left the index baseline, and this doc split off `build-velocity-shipped.md`. Pieces: 187 kept, 476 moved, 146 present, 2 dropped as stale; no issue filed.
- **Part 2** (done): every `app/FILES.md` entry cut the same way (202 entries, 261 lines), its detail moved into a new `//` block after the imports of the Swift file that defines its subject (301 files, comment lines only); the seven Swift comments citing `app/FILES.md` for a moved rule now cite `Sheets.swift`, `EditorViewController.swift` or `BackgroundWork.swift`. `scripts/check_docs_index_baseline.txt` and its code in `check_docs.py` are deleted, so every entry line of both indexes is checked strictly, and root `CLAUDE.md` says so. `verify_index_entries.py` gained `--comment-only` and the Swift rules (whole-file base comments, `+` in file names). Pieces: 248 kept, 496 moved, 4 present, 1 dropped as stale; no issue filed.

Verifiers (DIR holds the run's `core.jsonl` or `app.jsonl`): `python3 scripts/verify_index_entries.py --base c87ae93 --ledger DIR core` and `python3 scripts/verify_index_entries.py --base e028600 --ledger DIR --comment-only PATH … app` (the seven repointed files) each exit 0 with 10 `ok`; `python3 scripts/verify_status_split.py --base c87ae93 --added '## Phase 9c' build-velocity-status.md` (part 1) exit 0, 6 `ok`.

| Measurement | Command | Before | After |
|---|---|---|---|
| Core index bytes | `wc -c core/MODULES.md` | 278260 | 32203 |
| App index bytes | `wc -c app/FILES.md` | 217391 | 40192 |
| Entry lines over 200 | `python3 -c "print(sum(len(l)>200 for f in ('core/MODULES.md','app/FILES.md') for l in open(f,encoding='utf-8').read().split('\n')[4:]))"` | 284 | 0 |
| Baseline lines | `wc -l scripts/check_docs_index_baseline.txt` | 283 | deleted |
| Status doc bytes | `wc -c designs/build-velocity-status.md` | 39439 | 9288 |
