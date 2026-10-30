# Build velocity: implementation plan

Plan for the changes proposed in `designs/build-velocity-proposal.md` (the evidence and the measured
numbers are there). Each phase below is written to be handed to `/litecode` as one request. Work happens
on branch `worktree-modules-refactor` (worktree `.claude/worktrees/modules-refactor`), which was
fast-forwarded to main at `fe895fd` on 2026-10-06, one commit per phase.

## Decisions already made (owner, 2026-10-06)

- **The loop builds and drives a Debug app.** Release is built once per batch at the final gate. If an
  `assert` that Release compiles out fires in Debug, that is a defect to fix once, not a reason to avoid
  Debug.
- **`app/CLAUDE.md` must state the Debug caveats** (section "Phase 1", item 6).
- **The nested `CLAUDE.md` files are split** into short rules and a grep-only index (Phase 2).
- **README is updated by a periodic sweep; status docs keep per-change updates** but get a size rule, a
  check and a one-time condense-and-archive pass (Phase 9).
- **No crate or module split.** Nothing in this plan divides the Rust crate or the Swift target.
- The litecode skill and agent files in `~/.claude-personal` are shared with other projects and are
  **not** edited. Everything the pipeline needs is expressed through `make` targets and the repo's
  `CLAUDE.md` files.

## Evidence for designers

Outside the repo, so it survives the scratchpad: `~/.claude/litecode/rasterize/build-velocity/evidence/`

| Path | What |
|---|---|
| `results/*.md` | The seven investigation reports and four challenge reports (run-profile, pipeline-rules, rust-tests, rust-split, swift-loop, swift-split, practice-brief, challenge-*) |
| `earlier-reports/*.md` | Tooling, git-history, FFI-boundary and outside-research reports |
| `timing/harness.py`, `timing/extras.py` | The timing harness; `--only <step prefixes>` re-runs chosen steps |
| `timing/results/results.jsonl`, `logs/` | Every measurement quoted in the proposal |
| `work/<angle>/` | Each investigator's scripts and small data files |
| `transcript-scripts/` | Scripts that flatten session transcripts into per-command tables |

Every phase records its before and after timings in `designs/build-velocity-status.md` (created in
Phase 1): one table per phase, each number with the command that produced it.

## Before Phase 1 (orchestrator, no litecode)

- Delete the untracked `build-timing/` folder (apps left by the study's timing run, built from an
  older commit) so it is never committed or driven.
- Confirm the worktree is then clean apart from the two design docs; commit the docs with Phase 1.
- Never commit `Rasterize.xcodeproj`; delete stray `scripts/**/__pycache__` before `git add`.

---

## Phase 1 — Swift loop commands

**Goal.** The Swift check takes seconds, the loop builds a Debug app, and no path can drive or link a
stale binary.

**Why.** `make typecheck` 203 s → 11.9 s with batch mode; app build after a one-line Swift edit: Release
156 s, Debug 14 s (20–23 s when every file recompiles); a bare `xcodebuild` after a Rust edit skipped
the cargo step.

**Changes.**

1. `Makefile` `typecheck`: add `-enable-batch-mode -j <logical CPUs>` to the existing `swiftc
   -typecheck` line. Nothing else about the target changes.
2. `Makefile`: new `app-debug: rust project` running `xcodebuild -configuration Debug` with the same
   `-derivedDataPath $(BUILD_DIR)`; the product is `build/Build/Products/Debug/Rasterize.app`. `make
   app` and `make run` stay Release and unchanged.
3. `project.yml`, pre-build script "Build Rust Core": `basedOnDependencyAnalysis: false`, so cargo runs
   on every build (0.1 s when nothing changed).
4. Measure, then choose, two Debug settings passed on the `xcodebuild` line: `COMPILER_INDEX_STORE_ENABLE=NO`
   and `ENABLE_DEBUG_DYLIB=NO`. Keep a setting only if the incremental build is not slower with it and
   `Contents/MacOS/Rasterize` still launches under `RZ_BACKGROUND=1`.
5. Drive scripts: the scripts whose `--app` default is the Release product path (30, plus the launchers
   named in the root `CLAUDE.md`: `phase5_support.App`, `verify_phase1.instance`,
   `verify_phase9i_assisted.instance`, `host-source-probe/support.py`) take their default from one
   shared helper. The helper returns `RZ_VERIFY_APP` if set, otherwise the Debug product, and **refuses
   to launch an app binary older than** the newest of `app/Sources/*.swift`, `core/include/rasterize_core.h`
   and `core/target/release/librasterize_core.a`, naming the `make` target that rebuilds it.
   `RZ_ALLOW_STALE_APP=1` overrides. Scripts where `--app` is required are not touched.
6. Docs.
   - Root `CLAUDE.md`, Commands: add `make app-debug`; replace the two typecheck bullets with: IDE
     diagnostics are noise; `make app-debug` is the loop's Swift check and builds the app that drives
     use; `make typecheck` is a fast check with no artifact that stops before SILGen; `make app`
     (Release) runs once per batch at the final gate; do not run `make typecheck` when an app build
     follows. The launch recipe under "Driving a built app" uses the Debug path.
   - `app/CLAUDE.md`, a short "Debug and Release" rule: (a) the 58 `assert` / `assertionFailure` sites
     are live in Debug; a trap is a defect to fix, never an assert to delete or a reason to switch
     configuration; (b) Swift code that loops over a whole canvas runs unoptimised in Debug, so
     large-document drives are slower, and new per-pixel work belongs in Rust; (c) speed and memory are
     measured only on a Release app; (d) a failure that appears only in Release is found at the batch
     gate and is fixed there.
   - `README.md` "Building" block: the two build targets.
7. Create `designs/build-velocity-status.md` with the Phase 1 timing table.

**Acceptance.**

- `make typecheck` passes on a clean tree and exits non-zero, printing the error, with one injected
  type error; wall time recorded for both (expected about 12 s).
- `make app-debug`: cold, no change, after a one-statement edit in a leaf Swift file, after adding a
  top-level function to `RasterCore.swift`, after a comment-only edit to `rasterize_core.h`, and with
  one injected type error; wall time recorded for each (expected 33 / 2 / 14 / 23 / 20 / 17 s).
- After editing one file in `core/src`, a bare `xcodebuild -configuration Debug …` (not through `make`)
  rebuilds the library and relinks: the app binary is newer than the edit.
- The Debug app launches with the documented background recipe and answers `tools/list`.
- With a Swift file touched after the last build, a script using the shared default refuses to launch
  and names `make app-debug`; with `RZ_VERIFY_APP` pointing at the Release app it launches that.
- `make app` still builds Release; `.github/workflows/publish.yml` is untouched.

**Out of scope.** Fixing drives that fail against Debug (Phase 3); the gate script (Phase 4); probe
runners (Phase 6).

**Permitted cut.** Item 5 may become Phase 1b if the script edits make the run too large; the staleness
refusal must then ship in the launchers first.

---

## Phase 2 — Split the nested CLAUDE.md files

**Goal.** Every agent gets the rules; nobody is handed 100k tokens of file map.

**Why.** `core/CLAUDE.md` is 283 KB (about 4.5 KB of rules, the rest a 159-entry module map);
`app/CLAUDE.md` is 222 KB (about 13.5 KB of rules). Each is injected whole on the first tool call that
touches the directory, about 100k and 80k tokens, and carried to the end of the run: about 26% of
implementer input and 43% of fixer input. 29 of 70 implementers were never sent the core rules and 49 of
70 never the app rules.

**Changes.**

1. `core/CLAUDE.md` keeps only its Rules section. The module map, and the file entries that were
   appended inside the Rules section, move verbatim to `core/MODULES.md`.
2. `app/CLAUDE.md` keeps only its rules. The file map and the misplaced file entries move verbatim to
   `app/FILES.md`.
3. Both index files start with a three-line header: grep this file by file name, never read it whole; a
   new entry is one line of at most 200 characters; detail belongs in the source file's own header
   comment.
4. Root `CLAUDE.md`: replace "loaded on demand — read them before writing code on that side" with
   `@core/CLAUDE.md` and `@app/CLAUDE.md` imports, plus one line naming the two index files and the
   grep-only rule. Verify on one subagent transcript that the imported rules are present from the first
   turn; if imports do not reach subagents, keep the on-demand wording (the files are now small).
5. Every other reference to "the module map in `core/CLAUDE.md`" or "the file map in `app/CLAUDE.md`"
   in tracked docs, scripts and source comments points at the new files.

**Acceptance.** `core/CLAUDE.md` under 8 KB and `app/CLAUDE.md` under 16 KB; every entry of the old maps
is present, byte for byte, in the new index files (script check); no tracked file still describes the
maps as living in a `CLAUDE.md`; sizes and the transcript check recorded in the status doc.

**Out of scope.** Shortening existing entries to one line (Phase 9c); README and status docs (Phase 9a,
9b).

---

## Phase 3 — Qualify the Debug app

**Goal.** Know which existing drives pass against the Debug app, fix what Debug exposes, and pin the
drives that must stay on Release.

**Changes.**

1. Build Debug and Release from the same tree. Run every `scripts/` drive that takes `--app` or
   `RZ_VERIFY_APP` against both, except `*_ui.py`, Photoshop and GIMP drives, and probe runners (they
   compile their own app). A drive whose fixtures are not on this machine is recorded NOT RUN with the
   reason. If the full set would exceed the run, run first the drives that appear in handoff logs since
   2026-10-01 and record the rest as NOT RUN.
2. For each drive record pass or fail and duration on each configuration.
3. An assertion that fires in Debug: fix the defect it reports. If the assert itself is wrong, fix the
   assert and say why. Each fix gets a regression check where one is practical.
4. A drive that passes on Release and fails on Debug only for time, or runs more than about three times
   slower: do not optimise Swift in this phase. Add it to a "Release-only drives" list in the root
   `CLAUDE.md` (with `scripts/verify_phase4.py`, `scripts/phase4_raw_latency.py` and
   `scripts/raw-decode-probe/measure.py`, which measure time), and file an `ai-drafted` issue naming the
   Swift loop responsible.
5. A drive that fails on both is an existing failure: file an issue, do not fix here.

**Acceptance.** The table in the status doc covers every drive in scope; no drive fails on Debug with an
assertion; the Release-only list exists and each entry has a reason; issues are linked.

---

## Phase 4 — One gate, run once

**Goal.** One command that fits in a single tool call, runs only what the batch's changes can affect,
and records how long each step took.

**Why.** The full gate takes a median 813 s today, over the 600 s foreground cap. A batch averages 5
Swift checks, 2 app builds and 1.5 whole-suite runs. The whole-suite run is 497 s as cargo does it and
98 s with the same binaries four at a time.

**Changes.**

1. `scripts/run_tests.py`: builds with `cargo test --release --no-run --message-format=json`, then runs
   the test executables from `core/` four at a time. A "solo" list (the large-document binaries
   `filtered_source_ops_24mp`, `filtered_interactive_ceilings`, `live_filter_rank_band`,
   `live_filter_45mp`, `live_filter_admission`, and the binaries with wall-clock assertions that failed
   or are documented as load-sensitive, starting with `content_aware_tests`, `content_aware_cost_tests`
   and `heal_tests`) runs one at a time after the pool. It runs every binary even after a failure and
   ends with the list of failing binaries. Ignored tests stay ignored. `make test` calls it; `make
   test-serial` keeps plain `cargo test --release`.
2. `core/Cargo.toml` `[lib]`: `doctest = false` (there are no doc tests).
3. `scripts/gate.py` and `make gate`:
   - takes a lock under `build/` so two gates never overlap, and waits for a running `cargo` or
     `xcodebuild` started elsewhere;
   - computes a content hash of each leg's inputs and skips a leg whose hash equals the hash at its
     last pass (`--force` runs everything). Rust inputs: `core/` sources, tests, manifests. Swift
     inputs: `app/`, `core/include/rasterize_core.h`, `project.yml`, `Makefile`, and the built core
     library. A change to only `.md` files runs nothing;
   - runs `cargo fmt --check` and one `cargo build --release`, then two legs side by side: Rust
     (`cargo clippy --all-targets -- -D warnings`, then `scripts/run_tests.py`) and Swift (`make app`,
     then a background launch of the Release app that must answer `tools/list`);
   - prints one line per step (name, exit code, seconds, first error lines), writes full output to log
     files under `build/gate-logs/`, and appends one row per step to `build/gate-times.tsv` (UTC time,
     commit, step, seconds, exit code, Swift file count, test binary count).
4. Harden an assertion that failed twice in past whole-suite runs and passed on re-run: eight sites in
   seven test files assert that an error message does not contain "png" (any case) while the message
   ends with a random temp directory name such as `.tmpnGwRXe`. A deliberate attempt to reproduce it
   with "png" higher up the temp path did not fail, so first confirm what part of the path the message
   prints, then assert on the message with the path removed.
5. Root `CLAUDE.md`: a "Verification" section replacing recipe step 8's command list.
   - In the loop: targeted `cargo test --release --test <file> <filter>`, `make lint` after Rust edits,
     `make app-debug` after Swift or header edits, and the drives the change needs, against the Debug
     app.
   - Once, when the code is complete and before review: `make gate`. It is the only Full verification
     command; a spec lists it once, plus any Release-only drive the batch needs.
   - After a fix pass: `make gate` again. It re-runs only the legs whose inputs changed.
   - Never run the whole suite, `make typecheck` before an app build, or a gate step by hand as a
     substitute; never run a check twice to see its output differently.
   - When a refusal or error string changes, grep `core/tests` for the old wording before running
     anything (three of the five whole-suite-only regressions in three sessions were this).
6. `README.md`: the gate in the "Building" block.

**Acceptance.**

- Two-sided change: `make gate` wall time recorded (target under 5 minutes) and under the 600 s cap.
- No change since the last pass: every leg reports skipped. A `.md`-only change: nothing runs.
- Swift-only change: Rust legs skipped. Rust-only change: clippy and tests run, and the app is relinked.
- Each of these makes the gate exit non-zero and names the step: one failing test, one clippy warning,
  one Swift type error, an app that does not answer `tools/list`.
- `scripts/run_tests.py` three times in a row with no failure; wall time recorded beside `make
  test-serial`.

**Out of scope.** Merging test binaries (Phase 5); a typecheck leg for probe sources (Phase 6).

---

## Phase 5 — Test binaries: stop the growth, then merge without moving files

**Goal.** A new feature area no longer adds a binary, and the 400-odd binaries become roughly 60–70 with
no edit to any test file.

**Why.** About 90 new binaries a week. Every library edit relinks all of them (46 s), each freshly
linked binary costs about 0.33 s on first launch (135 s per whole-suite run), and the per-binary caches
are 6.9 GB of a 9.3 GB target directory. Prototype, files unchanged: 27 `psd*` files rebuilt in 4.4 s
merged against 15.2 s separate after a library edit, and ran in 3.5 s against 12.0 s; 90 `precision_*`
files 2.2 s against 9.3 s, and 12.1 s against 68.9 s, with 14 failures from tests that assert on
process-wide caches.

**Changes.**

1. `core/Cargo.toml`: `autotests = false` and explicit `[[test]]` entries.
2. Area suites as root files in `core/tests/` that mount existing files with
   `#[path = "<file>.rs"] mod <file>;` and carry `#![allow(clippy::duplicate_mod)]`. No test file is
   edited or moved. Suites by file-name family, none above about 50k lines; the designer produces the
   full assignment, including the files with no family prefix.
3. These stay as their own binaries in this phase (a `[[test]]` entry each, with a comment giving the
   reason):
   - the 8 files with `#[global_allocator]`, `process_working_limit`, `durable_save`;
   - every file that calls `rz_mixed_resources_evict_caches` or the helper that wraps it, or asserts on
     contiguous views, memo entries or `weak_count` (the file-local `SERIAL` locks stop protecting
     anything once files share a process);
   - every file with a wall-clock assertion in a default test;
   - the large-document files on the Phase 4 solo list.
   The evidence reports `results/rust-tests.md` and `results/challenge-test-merge.md` list them with
   file and line.
4. A guard test that fails when a `core/tests/*.rs` file containing `#[test]` is mounted by no suite and
   has no `[[test]]` entry, when a file is mounted twice, or when a `[[test]]` path does not exist.
5. `make t F=<file> T=<filter>`: finds the suite that mounts the file and runs
   `cargo test --release --test <suite> -- <file>::<filter>`.
6. `Makefile` `cancel-bench` and the Phase 4 solo list: new binary names, with module filters where a
   named file now lives in a suite, so a suite's other ignored tests do not run.
7. Rules. `core/CLAUDE.md`: replace "new feature area = new test file" with: a new area is a new test
   file mounted in its family's suite; a new `[[test]]` entry needs a stated process-isolation reason;
   a test that asserts on process-wide state gets its own binary. Root `CLAUDE.md`: the single-test
   command becomes `make t`.

**Acceptance.**

- Keyed comparison before and after: for every test executable, `<executable> --list --format terse`
  and again with `--ignored`; before, key each name by its binary; after, suite names already begin
  with the file's module. The two sets are equal, with no key twice.
- `make gate` clean; `scripts/run_tests.py` three times in a row with no failure.
- Recorded before and after: test build after a library edit, whole run wall time, size of
  `core/target/release`, number of binaries.

**Permitted cut.** Two runs: rule, guard, wrapper and the first families; then the remaining families.

**Out of scope.** Editing test files to share `tests/common` once per suite, a shared lock for the
cache-asserting files, and converting wall-clock assertions (Phase 10).

---

## Phase 6 — Probe runners share one cached build

**Goal.** A probe run compiles the app at most once per source state.

**Why.** All 20 runners compile every app source into a fresh folder with a fresh module cache on every
run. `scripts/jobs-probe/verify.py` alone ran 64 times for 5.6 h in one session, and agents type-check
probes by hand with the slow single-file form (120–600 s).

**Changes.**

1. `scripts/probe_build.py`, used by all 20 runners in place of their inline `swiftc` call: a
   persistent Clang module cache under `build/`; the existing flags plus `-num-threads`; the linked
   probe stored under `build/probe-cache/<hash>` where the hash covers every input file's content, the
   flags and the core library, and reused on a hit; prints "reused build <hash>" or "built <hash>".
2. `python3 scripts/probe_build.py --typecheck <probe dir>`: a batch-mode typecheck of the app sources
   with that probe's sources.
3. `scripts/gate.py`: a probe leg that type-checks each probe directory whose files changed in the
   batch.
4. The runners' provenance hashes keep covering the same inputs.

**Acceptance.** One runner: wall time of a first run, an unchanged re-run, and a run after one Swift
edit, recorded against today's. Editing any input changes the hash (checked for an app source, a probe
source and the library). All 20 runners build through the helper.

---

## Phase 7 — Hub files by area

**Goal.** A feature edits small files of its own instead of five large shared ones. This is for smaller
reads, smaller edits and fewer collisions; it is not a build-time change.

**7a, mechanical.**

- `RasterCore.swift` (7,889 lines) divided along its `MARK` sections into `RasterCore+<Area>.swift`
  files; the handle classes and error plumbing stay; the handful of `private` helpers used across
  sections become internal. No caller changes.
- `AgentCatalog.swift` (3,999 lines): each `<area>Catalog` function moves to
  `AgentCatalog+<Area>.swift`.
- Rules: recipe step 2 names `RasterCore+<Area>.swift`; `rz_*` calls stay confined to `RasterCore*`
  files; the frozen-file list in `app/CLAUDE.md` is updated.
- Acceptance: the MCP `tools/list` output is byte-identical before and after; `make gate` clean.

**7b, registries.**

- `AgentServer.handlers` becomes per-area tables declared in each `AgentServer+<Feature>.swift`, merged
  in one short registry file. The same shape for `ActionSteps.swift`, the `DocumentPrecision`
  whitelists and menu construction in `AppDelegate.swift`.
- The catalog/handler parity check runs at server start in both configurations and fails loudly.
- Acceptance: `tools/list` identical; the menu tree (titles, order, key equivalents) identical; the
  recorded-step and whitelist tables equal as sets; `make gate` clean.

---

## Phase 8 — Rust build settings and a layering rule

1. `image` with explicit features: drop the AVIF encoder if no save path uses it. Removing any decoder
   changes which files open and needs the owner's decision; list them, do not remove them.
2. Move the two in-library unit tests (`doc_comps.rs`, `rzdc_capacity.rs`) into `core/tests`; set
   `[lib] test = false`.
3. The release-profile comment in `core/Cargo.toml` cites a document from another repository; rewrite
   it to say what the profile does here and that the shipped library is built with it.
4. `core/CLAUDE.md` rules: shared limits live in a leaf module that imports nothing from the crate; a
   feature's pixel math is free functions in a kernel module that does not name the document types or
   the hub types (`PrecisionImage`, `Adjustment`, `LayerStyle`, `IccProfile`, `resources::Inventory`);
   the feature's `impl` block is a thin adapter. `scripts/check_layers.py`, run by `make lint`, enforces
   a list of kernel modules that already comply.
5. Acceptance: cold `cargo build --release --timings` before and after item 1; `make gate` clean; the
   lint fails on a deliberate violation.

**Out of scope.** A loop profile separate from the shipping profile (the `release` directory name is
hard-coded in `project.yml` and 24 scripts); relocating existing items to satisfy the layering rule.

---

## Phase 9 — Documents: README by sweep, status docs kept small

Owner decisions (2026-10-06): README may be updated less often. Status docs keep their per-change
updates, because an agent updating them later would not have the detail. The detail they have
accumulated does not need to stay in the working docs, and the condense-and-archive pass of 2026-09-26
(commit `c6cfa65`: 87 originals moved to `archive/`, each condensed into an ADR at the same basename,
indexed in `archive/INDEX.md`) was never made a rule, so later phases grew large docs again.

Sizes today: 58 status docs, 1.7 MB. `phase-13b-tiled-status.md` is 527 KB and
`phase-13a-regional-status.md` 475 KB; four more are between 43 and 96 KB. README is 222 KB and is
edited by 89% of commits; `.md` files hold 21 of 53 actionable review findings.

**9a — README by sweep.**

- Root `CLAUDE.md`: the recipe no longer says "Update README". A batch does not edit `README.md`,
  except for a statement in it that the batch makes false.
- A sweep brings README up to date from the commits since the last sweep, reading each commit's message
  and its status-doc entry, and the MCP tool list from a running app. It records the commit it swept
  to. It runs before every push to `publish` and at least every 20 commits.
- Acceptance: the rule text; one sweep run from the commit where per-change README edits stop.

**9b — Status docs: a rule, a check, and one clean-up.**

1. Rule, in the root `CLAUDE.md` where it already says design decisions are condensed ADRs:
   - A status doc is the working record of an **open** phase and holds only what a later batch needs:
     decisions in force, the contract of each sub-slice not yet shipped, open items linked to their
     issues, and for each shipped sub-slice a few lines (what shipped, the commit, anything a later
     slice must know). It is updated by the batch that changes any of those, as today.
   - Run logs, receipts, measurement tables, review histories and superseded designs do not go in it.
     They stay in the handoff directory and the receipts folders outside the repo, or in the commit
     message.
   - When a sub-slice ships, its section is cut to those few lines in the same batch. When a phase
     closes, the closing batch condenses the doc into an ADR at the same basename, moves the original
     to `archive/`, and adds its line to `archive/INDEX.md`.
   - A status doc stays under 40 KB.
2. Check: `scripts/check_docs.py`, run by `make gate`, fails when a `designs/*-status.md` exceeds the
   cap, when `core/CLAUDE.md` or `app/CLAUDE.md` exceeds its Phase 2 size, or when a new index entry in
   `core/MODULES.md` or `app/FILES.md` is longer than 200 characters. Without a check the rule will
   lapse as it did after September.
3. Clean-up, following the 2026-09-26 procedure: every status doc over the cap, and every status doc of
   a phase the roadmap marks shipped, is condensed; the original moves to `archive/` verbatim; each
   open item found is checked against the code and existing issues, then filed with the `ai-drafted`
   label and linked. For a phase that is still open (13A waits on its latency item; 13B is in
   progress), the sub-slices already shipped are condensed and the open contracts are kept in full.
   One run for the rule, the check and the mid-sized docs; one run each for the 13A and 13B docs.
4. Acceptance: no status doc over the cap; `make gate` runs the check and it fails on a deliberate
   oversize; every archived original is byte-identical to its last version in `designs/`; every ADR's
   open items are issue links.

Do 9b before Phase 5 if feature work will resume in between: the two largest docs hold 453 `--test`
names that Phase 5 makes stale.

**9c — One-line index entries.** Shorten the entries of `core/MODULES.md` and `app/FILES.md` to one
line each, moving detail worth keeping into the source file's own header comment.

---

## Phase 10 — Follow-ups, each small enough to be an issue

- Compile `tests/common` once per suite (`use crate::common;` in the 199 files that declare it).
- A serial suite with one shared lock for the cache-asserting files, so they stop needing their own
  binaries.
- Convert the default-run wall-clock assertions (26 files) to the park seam or to `make cancel-bench`
  probes.
- Re-profile after about 20 batches with the transcript scripts and `build/gate-times.tsv`; compare
  with the proposal's table.

## Owner actions outside the pipeline

- One-time experiment: add the terminal that runs the agents to System Settings → Privacy & Security →
  Developer Tools, rebuild, and time `make test-serial`. If the roughly 135 s first-launch cost of new
  test binaries disappears, keep the setting.
- Say whether `archive/` (1.9 MB today) should stay in the repository. The plan keeps it there, as the
  September pass did.

## Order and dependencies

1 → 2 → 3 → 4 → 9b → 5 → 6 → 7a → 7b → 8 → 9a → 9c → 10. Phase 1 comes first because it shortens
every later run; Phase 2 second because it lightens every later agent. Phase 3 needs Phase 1. Phase 4
needs Phase 1 and should follow Phase 3 so the gate's drives are qualified. Phase 9b needs Phase 4's
gate for its check and goes before Phase 5 so the condensed docs do not carry test names that Phase 5
changes. Phase 5 needs Phase 4's runner. Phases 6, 7 and 8 are independent of one another.
