# Build velocity: the change loop's build, test and docs tooling (ADR)

- **Status:** Accepted — closed; plan complete 2026-10-08
- **Phase / dates:** 2026-10-06 to 2026-10-08, branch `worktree-modules-refactor`
- **Originals:** [archive/build-velocity-proposal.md](../archive/build-velocity-proposal.md) — the study and its measurements; [archive/build-velocity-plan.md](../archive/build-velocity-plan.md) — the plan, phase by phase; [archive/build-velocity-status.md](../archive/build-velocity-status.md) — the working record at close (Phases 9b and 10 in full); [archive/build-velocity-shipped.md](../archive/build-velocity-shipped.md) — its shipped companion (Phases 1 to 9a and 9c, every table); [archive/2026-10/build-velocity-status.md](../archive/2026-10/build-velocity-status.md) — Phases 1–4 as first recorded, with the per-drive Debug/Release table
- **Evidence:** `~/.claude/litecode/rasterize/build-velocity/evidence/` — reports, the timing harness, and the `transcript-scripts/` a re-profile would re-run
- **Key commits:** listed in each phase section below

## Context
A profile of the Oct 2–6 session (70 batches, 4.31 days, 102 h of active agent time) found the loop slow because of how the
compilers and tests were invoked, not because each side is one module: `make typecheck` took a median 216 s and ran about 5 times a
batch, the whole suite ran 1.5 times a batch (497 s as cargo ran it), and the full gate (median 813 s) did not fit one tool call. What
grew was the number of test binaries (about 90 a week), the two nested `CLAUDE.md` files (283 KB and 222 KB) and the hub files.

## Decisions
- **Owner, 2026-10-06 (the plan's six):** the loop builds and drives a Debug app, and Release is built once per batch at the final
  gate; an `assert` that fires in Debug is a defect to fix once, not a reason to avoid Debug. `app/CLAUDE.md` states the Debug caveats.
  The nested `CLAUDE.md` files are split into short rules and a grep-only index. README is updated by a periodic sweep; status docs keep
  per-change updates under a size rule, a check and a one-time condense-and-archive pass. No crate or module split. The shared litecode
  skill and agent files in `~/.claude-personal` are not edited; the pipeline is expressed through `make` targets and the repo's
  `CLAUDE.md` files.
- **Owner, Phase 9 (2026-10-06):** README may be updated less often (by sweep); status docs keep their per-change updates, because a
  later agent would not have the detail.
- **Owner, 2026-10-07:** archives stay in the repository ("Archives stay in repo").
- **P1:** `make app-debug`, the IDE-diagnostics bullet and `make typecheck` (stops before SILGen) are root `CLAUDE.md` "Commands"; no
  `make typecheck` before an app build is its "Verification"; the stale-app refusal (`DR_ALLOW_STALE_APP`, `DR_VERIFY_APP`) is its
  "Driving a built app for verification"; what a clean typecheck does not prove and the Debug caveats are `app/CLAUDE.md`'s
  "`make typecheck` is authoritative for TYPE errors" and "Debug and Release".
- **P2:** root `CLAUDE.md` imports `core/CLAUDE.md`, `app/CLAUDE.md` and `mcp-proxy/CLAUDE.md`; the per-file indexes `core/MODULES.md`
  and `app/FILES.md` are grep-only (root `CLAUDE.md`, the paragraph after the imports, which `core/CLAUDE.md` and `app/CLAUDE.md`
  point to, and each index's header).
- **P3:** the Release-only drives are root `CLAUDE.md` "Release-only drives".
- **P4:** root `CLAUDE.md` "Verification" and `make gate` (`scripts/gate.py`): its hash inputs are `RUST_INPUTS` and `SWIFT_INPUTS` in
  `gate.py`, a `.md`-only change runs only the `docs` step, and the solo list is `SOLO` in `scripts/run_tests.py` (`core/CLAUDE.md`
  "Tests." says when a binary joins it).
- **P5:** the suites (`core/tests/suite_*.rs`), `autotests = false`, the `# isolate:` reason on a `[[test]]` entry, `make t` and the
  `test_layout` guards are `core/CLAUDE.md` "Tests." and root `CLAUDE.md` "Commands".
- **P6:** `scripts/probe_build.py` (its cache key covers every input, per its module docstring), the gate's `probes` step and
  `DR_PROBE_BUILD_ONLY` are root `CLAUDE.md` "Verification"; a probe set whose basenames clash by case builds without `-num-threads`
  (`case_clashes` in the helper).
- **P7a:** `DoctoredCore+<Area>.swift` (root `CLAUDE.md` "FFI contract", recipe step 2), `AgentCatalog+<Area>.swift` and `dr_*`
  confined to the `DoctoredCore*` files are `app/CLAUDE.md` "Placement."; the frozen six are `app/CLAUDE.md`'s header and its
  `app/FILES.md` entries.
- **P7b:** the per-file handler tables, the `ToolFacts` tables and the start parity check are `app/CLAUDE.md` "Agent parity." (root
  `CLAUDE.md` recipe step 6 points there); the menu builders are `app/CLAUDE.md` "Placement." (recipe step 5); `DR_REGISTRY_DUMP`
  (`RegistryDump.swift`) is Debug-only.
- **P8:** `image` takes explicit features without `avif`, `[lib] test = false` and no LTO are `core/Cargo.toml` (its comments say why);
  the kernel layering rule is `core/CLAUDE.md` "Layers.", enforced by `scripts/check_layers.py` under `make lint`.
- **P9a:** the README sweep is root `CLAUDE.md` "README" (`scripts/readme_sweep.py`).
- **P9b:** the status-doc rule, the shipped companion and the `check_docs.py` caps are root `CLAUDE.md` "Status docs".
- **P9c:** index entries are one line of at most 200 characters, detail in the source file's own header (each index's header,
  `core/CLAUDE.md` and `app/CLAUDE.md`; `check_docs.py` checks every entry line, root `CLAUDE.md` "Status docs").
- **P10:** shared test modules declared once per suite root, the `suite_serial` lock as every test's first statement, `mod bench` twins
  run by `make cancel-bench`, and `test_layout::no_default_test_asserts_a_duration` are `core/CLAUDE.md` "Tests.".
- **`eaa4d2c`:** a small fix whose answer is clear is made in place, not filed (root `CLAUDE.md` "Architecture", the open-work paragraph).
- **Keeping it fast (proposal §5):** the gate's timing log (`build/gate-times.tsv`) is the control. A look is due when the Swift check is
  over 30 s, the full gate over 8 minutes, a whole-suite run over 3 minutes (already over at close: 240.3 s), or a nested
  `CLAUDE.md` over 20 KB. No scheduled re-profile (dropped, Open items).

## Rejected alternatives
- **A Rust crate or Swift module split for build speed.** Whole-unit rebuilds were already short once invoked properly (library 12 s,
  Swift Debug 14–23 s), and the code that changes most is what everything depends on. Revisit on the proposal's triggers: a full Debug
  recompile over about a minute, a library rebuild over about 30 s, the Release build becoming the gate's longest step, or kernel-only
  edits above a quarter of Rust builds with tests allowed to link kernels directly.
- **A loop profile separate from the shipping profile.** The `release` directory name is hard-coded in `project.yml` and 24 scripts.
- **Removing `image` decoders without the owner.** That changes which files open.
- **Editing the shared litecode skill files.** They serve other projects; everything goes through `make` and the repo's `CLAUDE.md`.

## Shipped runs
The phase sections follow, in number order.

## Phase 1
`8384ac9`: `make typecheck` in batch mode; `make app-debug`, the Debug app the loop and the drives use; cargo runs on every Xcode build
(`basedOnDependencyAnalysis: false`), so a bare `xcodebuild` after a Rust edit relinks; `scripts/app_under_test.py` gives the drives
their default app and refuses a stale one. `COMPILER_INDEX_STORE_ENABLE=NO ENABLE_DEBUG_DYLIB=NO` are kept as "not slower": the product
has no `Doctored.debug.dylib`, and the first build after a settings change recompiles every file.

## Phase 2
`6630ce3`: the rules stay in `core/CLAUDE.md` and `app/CLAUDE.md`; every per-file entry moved verbatim to `core/MODULES.md` and
`app/FILES.md` (`scripts/verify_claude_md_split.py --base 8384ac9`). The import form reaches a subagent's first turn. Dated slice records
and `archive/` were not rewritten.

## Phase 3
`c46f334`: 100 drives ran on Debug and Release from `6630ce3`: 77 pass on both, 1 Release-only, 11 fail on both, 11 not run. No Debug
run trapped, so the 58 asserts stand; the drives default to Debug. Release-only: `verify_phase12g_clipboard.py`, which now passes
on Debug. Fail on both, not fixed here: the 8A, 8B, 8C, 9B, two 10D, 11D, 11E, 12A, 12F-1 and Phase 4 drives (each fixed or dropped
since, Open items); the 11 not run have no inputs here; `verify_phase12f3_warp.py` rewrites a tracked page.

## Phase 4
`4a3c70e`: `make gate` (`scripts/gate.py`), the parallel runner `scripts/run_tests.py` (`make test`; `make test-serial` stays plain
cargo) and the "Verification" rule. The gate returns non-zero for any outcome but a pass and names it on its last line; `core-build`
runs on every run past `wait`, so the hashed library is the one the app links. Relinked test binaries run about 2.5× slower the first
time, which kept the gate after a `core/src` edit over 300 s until Phase 10. The "png" assertions needed no change.

## Phase 5
`bad6dcc`: 407 one-file test binaries became 8 suites mounting 323 files by `#[path]` plus 84 standalone binaries, with no test file
edited; `scripts/verify_test_merge.py` keyed the same 3,932 tests before and after. `make cancel-bench` runs its files through
`scripts/run_test_file.py`. The test files' stale `cargo test --test <file>` comments and ignore reasons became `make t` commands
in the batch that wrote this ADR; the generator commands in five fixture headers are history, run at the commit they name, and were
left as written.

## Phase 6
`fa47689` (6a), `ad33e84` (6b): all 20 of the plan's probe runners build through `scripts/probe_build.py`, which keeps the linked probe
in `build/probe-cache/<key>/` (sixteen entries since 2026-11-12, eight before) and a persistent module cache. The gain is the reuse
on unchanged sources, not the compile. The `probes` gate step type-checks changed or last-failed probe directories only, so a committed one is checked by hand.
Existing failures found: `color-metadata-probe` and `test_provenance_manifests.py` (fixed since) and `verify_phase8c.py --host-only`,
whose host probe hung; it passed 2026-11-12 under `DR_BACKGROUND=1`, the hang's cause unrecorded.

Since 2026-11-12 the gate's `precision-probe` step runs `scripts/precision-probe/verify.py`'s small run (no flag): no gate ran it,
and its oracles went stale unseen until a batch ran it by hand. A gate step, not a periodic runner, because the tree has no periodic
probe runner and a new one would have no owner. The probe compiles under `DR_PROBE_BUILD_ONLY=1` beside the legs and runs under
`DR_BACKGROUND=1` once every leg has ended, since its verdicts include wall-clock bounds; the step is skipped while the app sources,
the header, the probe directory, the sources of `app/Info.plist` and the core library are unchanged since its last pass. Another runner's small run
would be one more `PROBE_RUNS` entry in `gate.py`. The `--large*` modes measure time and memory on 24 MP sources and stay out of the
gate. `probe_build.py` makes `build/probe-module-cache` again when it was made at another path, since a `build/` copied from another
checkout failed every probe compile. `make gate GATE_ARGS=--force` with the cached probe moved aside: the step 103.9 s (compile 99.1 s
beside the suite's 110.2 s, run 4.8 s), `gate: PASS 146.6s`; with the probe cached, `make gate` ran the step in 5.3 s.

Also since 2026-11-12, nine runners' small runs are `PROBE_RUNS` steps: `precision-probe`, `history-probe`,
`selection-history-probe`, `selection-probe --synthetic` (it skips the three checks that read private photo folders),
`save-preservation-probe`, `color-metadata-probe`, `data-migration-probe`, `path-guard-probe` and `updater-probe`. The eight that
joined read only the tree and system profiles and never came to the front under `DR_BACKGROUND=1` in any watched run; the precision
probe did, for a second, until it stopped calling `finishLaunching` (which activates the app) under that variable. Each takes
`DR_PROBE_BUILD_ONLY` itself (`probe_build.take_build_only`) and is keyed on its directory, the helpers it loads, and `project.yml`
and `VERSION` for the generated, git-ignored `app/Info.plist`, which `core-build` (now `make rust project`) writes before any leg
reads it. Left out: `jobs-probe`, which has no small run (about 234 s, reads `~/devel/doctored-fixtures`, has `ui_*` event
cases); the sets that are not verifiers (`screens-probe`, `reach-probe`, `raw-local-probe`, `artwork-text-probe`,
`raw-decode-probe`, `model-fixtures`); and the eight run only from a `scripts/*_ui.py` drive, which takes the focus. Joining found two stale oracles, fixed: Crop to Selection and a half turn carry the selection now,
and the PSD evidence's native reader peels the Background mark word 113. Each build is a whole-app compile, so at most
`PROBE_BUILD_JOBS = 2` run at once, and the cache keeps 16 builds so all nine survive to their runs (the gate refuses more runs
than that). `make gate GATE_ARGS=--force` with the probe cache moved aside: at 2, `gate: PASS 685.7s` (the app built warm, 8.0 s;
the nine builds over 12–484 s; each step 85.2–137.1 s); at 1, 1209.5 s (a cold app build, 471.3 s; the builds over 13–1000 s).
After a Swift change the probe compiles are the gate's critical path, past the 8-minute look.

## Phase 7a
`6e5d2e6`: `DoctoredCore.swift` keeps the handle classes and error plumbing; its other sections moved verbatim into 44
`DoctoredCore+<Area>.swift` files, and `AgentCatalog.swift`'s 14 area builders into `AgentCatalog+<Area>.swift`. 36 declarations lost
`private` because their callers moved; `tools/list` is `cmp`-identical. Four server-lifecycle `dr_*` calls remain in `AgentServer.swift`
(none since the socket batch, [agent-socket.md](agent-socket.md)).

## Phase 7b
`b7cfc7f`, `5837a08`, `814da95`: 77 per-file `<prefix>Handlers` tables merged by `AgentServer+Registry.swift`; 11
`AppDelegate+<Menu>Menu.swift` builders ordered by `buildMainMenu()`; 77 `<prefix>ToolFacts` tables, index-paired with the handler
tables, from which the nine tool-name sets are built (`coreToolFacts` is in `AgentServer+CoreToolFacts.swift`, since `AgentServer.swift`
is frozen). `enforceCatalogParity` exits 1 at start in Debug and Release on a catalog/dispatch mismatch, a duplicate, an unpaired facts
table, or a broken set rule. Not checked: `commonTools` and `typedTools` disjoint, `FloatDocument.tools` inside their union.

## Phase 8
`5cad1a7`: `image` takes image 0.25's `default` minus `avif`; the two library unit tests moved to `core/tests` behind `#[doc(hidden)] pub`
seams, and `test_layout::no_library_source_holds_a_unit_test` guards it; the `[profile.release]` comment says why there is no LTO;
`check_layers.py` covers 55 kernel modules. The lock keeps `ravif`/`rav1e` through weak features, unbuilt. Which decoders to keep is
the owner's decision.

## Phase 9a
`c87ae93`: the README rule and `scripts/readme_sweep.py`; README's last line is the marker `<!-- readme-swept-to: <sha> -->`. The first
sweep read `fe895fd..5cad1a7` and regenerated the stale MCP reference page.

## Phase 9b
`1e2f1bd`, `c7f4dcb`, `6f097b0`, `f182a99`, `4c3b22b`: the "Status docs" rule, `scripts/check_docs.py` as the gate's `docs` step and
`scripts/verify_status_condense.py`; 9F, Phase 4, Phase 9, 12G and nine shipped-phase ADRs condensed, 13A and 13B condensed as open
phases and then split into status doc and shipped companion (`scripts/verify_status_split.py`); originals in `archive/2026-10/`. The
40 KB cap has no exception.

## Phase 9c
`e028600`, `d300980`: every `core/MODULES.md` and `app/FILES.md` entry is one line of at most 200 characters, the detail moved into the
named file's own header comment (`scripts/verify_index_entries.py`); the index baseline file is deleted, so every entry line is checked.

## Phase 10
`df5af6d`, `eb55053`, `48662ae` and `b09b28f`: each suite root declares the shared test modules once; the 29 cache-state files share
`suite_serial` and its lock; default cancel tests park the worker (`common::probe::entry_then_cancel`, `cancelled_parked_at`) and every
timed body is a `mod bench` twin under `make cancel-bench`; 25 test executables remain. The gate after a `core/src` edit is 280.1 s,
under the 300 s target. The report-log PIN runs only in `#[ignore]`d twins and the guard counts a call to `assert_pin` / `assert_ramp`
(`CLOCK_VERDICTS`) in default code; each regional concurrent cancel parks the worker by default (`common::probe::cancelled_past_each`,
counts from a fresh twin) and its timing is an `…_is_measured` twin in `CANCEL_BENCH_FILES`. In `live_filter_native.rs` the two
parked-edit workers hold `common::probe::ReleaseOnDrop`, so a panicking edit still wakes the waiter (that file went with live filters
on 2026-10-10).

## Before and after
| Measurement | Command | Before | After | Phase |
|---|---|---|---|---|
| Type check, clean tree | `make typecheck` | 203 s | 12.6 s | 1 |
| App build, one statement changed in `app/Sources/CropTool.swift` | `make app` before, `make app-debug` after | 156 s | 14.3 s | 1 |
| `core/CLAUDE.md` / `app/CLAUDE.md`, bytes | `wc -c` | 282663 / 222189 | 4856 / 14809 | 2 |
| `core/MODULES.md` / `app/FILES.md`, bytes | `wc -c` | 278260 / 217391 | 32203 / 40192 | 9c |
| Test executables | `python3 scripts/run_tests.py`, last line (P5); `python3 scripts/verify_test_merge.py --write …` (P10) | 408 | 25 | 5, 10 |
| Test build after a library edit | `cd core && cargo test --release --no-run` after a `#[used]` static changes `core/src/durable_file.rs` | 46 s | 23 s | 5 |
| `core/target/release` after a clean build | `du -sk core/target/release` | 9,490,176 KB | 4,943,768 KB | 5 |
| Whole suite | `python3 scripts/run_tests.py` (P10 with `--summary …`) | 215.0 s | 240.3 s | 4, 10 |
| Gate after a `core/src` edit | `make gate` | `gate: PASS 413.3s` (P5 before) | `gate: PASS 280.1s` (P10 after) | 5, 10 |
| Probe run, sources unchanged | `python3 -B scripts/history-probe/verify.py --keep`, second run | 66.6 s | 4.6 s | 6 |
| Cold core build | `cd core && CARGO_TARGET_DIR=<scratch> cargo build --release --offline --timings` | 51.2 s | 43.4 s | 8 |
| `DoctoredCore.swift`, lines | `wc -l app/Sources/DoctoredCore.swift` | 7,889 | 1,178 | 7a |
| 13A / 13B status doc, bytes | `wc -c designs/phase-13a-regional-status.md designs/phase-13b-tiled-status.md` | 486083 / 540587 | 14636 / 20211 | 9b |

The whole suite got slower: 215.0 s at Phase 4, 240.3 s at close, 25.3 s more. Since Phase 5 the eight suite binaries run one at a
time instead of sharing the pool (Phase 5 measured 234 s, about 20 s over Phase 4). Phase 10's own runs moved between 218.4 and
240.3 s (227.0 s before its last two runs), one run each, with an outside build possibly overlapping the last.

## Open items
None. `DROPPED`: the re-profile after about 20 batches, the owner's Developer Tools first-launch experiment, the
owner's choice of which `image` decoders to keep, the 8B and 8C drives' frozen tool-catalog count, and the eleven drives with no
inputs on this machine. Every failure Phases 3 and 6 found was fixed, and the 8C host probe's hang did not recur
([phase-8g-raw-enhancements-status.md](phase-8g-raw-enhancements-status.md), 2026-11-12).
