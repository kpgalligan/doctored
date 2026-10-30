# Build velocity: where the change loop's time goes, and what to change

Study of 2026-10-06, at `22852ab`. Status: **proposal, nothing adopted**. No issues are filed yet, so
the backlog run cannot pick any of this up before it is approved.

## 1. Verdict

1. The loop is slow because of how the compilers and tests are **invoked**, not because each side is one
   module. Three command-level changes remove most of the build time in a batch.
2. The pipeline also **repeats** its gates: about 5 Swift checks, 2 app builds and 1.5 whole-suite runs
   per batch, and the full gate is too long for one command, so agents background it and poll.
3. **Do not split the Rust crate or the Swift module for build speed now.** Measured rebuilds of the
   whole unit are already short once invoked properly (Rust library 12 s, Swift Debug 14–23 s), and the
   code that changes most is the code everything depends on, so a split would rebuild nearly everything
   anyway.
4. What does need structural work is what **grows**: the number of test binaries, the size of the two
   nested `CLAUDE.md` files, and the hub files every feature edits.

Estimated effect of sections 3.1 to 3.3 together: about 33 minutes of build work per batch falls to
about 7, against 87 minutes of active agent time per batch today (section 2.3).

## 2. Evidence

### 2.1 Where agent time went (session of Oct 2–6: 70 batches, 4.31 days)

"Active" removes transcript gaps over 15 minutes. Source: transcripts of 516 subagents, audited twice.

| Measure | Value |
|---|---|
| Active agent time | 102 h (1.46 h per batch; the session before was 1.03 h per batch at twice the batches per day) |
| Implementer run | median 43 min: 32% model turns, 20% `make typecheck`, 24% waiting or polling on background builds |
| `make typecheck` | 348 executions (321 by subagents, 27 by the orchestrator), median 216 s, about 20 h; about 90% pass |
| `make app` | 155 builds, about 118 of them real rebuilds at a median 185 s; 106 of 143 followed a typecheck with no Swift edit between |
| Whole-suite `make test` | 102 runs in 61 of 70 batches; agents waited a median 391 s, 460–549 s in the last two days |
| Full gate in one command | median 813 s, over the 600 s foreground cap, hence 17 h of wait calls |
| Fix rounds | 31 of 70 batches (14% of batches one session earlier); 30 of 38 fixer runs re-ran the whole suite |
| All Rust compile steps | 1–4 h of the 102 |

Growth per day over Sep 22 – Oct 6: typecheck +4.3 s (linear in file count), whole suite about +25 to
+30 s, `make app` flat at about 185 s.

### 2.2 Measured on this machine (M2 Max, 12 cores; one run each unless noted)

Results and logs: `core/target/timing-2026-10-06/` in the `modules-refactor` worktree. Load from the
study's own agents was present during part of the run; the Rust steps were repeated afterwards and the
repeat is what is quoted.

**Swift check, 655 files**

| Command | Time | With one type error |
|---|---|---|
| `make typecheck` as it is (656 compiler jobs, one at a time) | 203 s | 145 s |
| the same plus `-wmo` | 31 s | 31 s |
| the same plus `-j 12` | 26 s | |
| the same plus `-enable-batch-mode -j 12` | **11.9 s** | 11.8 s, exit 1, error shown |

**App build** (`xcodebuild`, separate derived-data folder)

| Case | Debug | Release |
|---|---|---|
| Cold | 33 s | 171 s |
| Nothing changed | 1.5 s | 1.3 s |
| One statement changed in a leaf Swift file | **14 s** (1 file recompiled) | **156 s** |
| One statement changed in `RasterCore.swift` | 14 s | |
| Function added to `RasterCore.swift` | 23 s (642 files recompiled) | |
| Comment added to `rasterize_core.h` | 20 s (all 655 recompiled) | |
| One deliberate type error | about 17 s to fail | |

Release is one single-threaded compile task of 152–160 s; dSYM generation is 1.2 s. The Debug app
launched in the background and answered `tools/list` (269 tools) in 1.3 s with no assertion firing.

**Rust** (release profile, as `make test` uses)

| Case | Time |
|---|---|
| Library after a body edit in `doc.rs` | 12 s |
| then one test binary | 14 s |
| then all 407 test binaries | 46 s |
| One test file edited, its own binary | 2 s |
| `tests/common/mod.rs` edited, all binaries | 24 s |
| `make lint` after a `doc.rs` edit | 13 s |
| `cargo check --lib` after a `doc.rs` edit | 2–3 s |
| Dev profile: library plus one test binary after a `doc.rs` edit | 4 s |
| Cold library build, empty target directory | 46 s (the crate itself 26.5 s, third-party crates the rest) |

**Test run**

| Case | Wall time | Test time inside it |
|---|---|---|
| `cargo test --release`, everything built, binaries freshly linked | 497 s | 362 s |
| The same 407 binaries run 4 at a time, not freshly linked | **98 s** | 378 s |
| 10 at a time | 79 s, and `content_aware_tests` failed a wall-clock assertion | |

A freshly linked test binary costs about 0.33 s on its first launch (135 s across 407; 0.31 s each in a
27-binary sample) and about 0.03 s on later launches. Every library edit relinks all 407, so every
whole-suite run after a change pays it. The cause was not established; it matches what the nextest
documentation attributes to macOS scanning new executables.

**Merged test binaries, prototype with no edits to test files** (a root file that mounts the existing
files with `#[path]`)

| | 27 `psd*` files (18.5k lines) | 90 `precision_*` files (55.7k lines) |
|---|---|---|
| Rebuild after a library edit: separate / merged | 15.2 s / 4.4 s | 9.3 s / 2.2 s |
| Rebuild after editing one test: separate / merged | 0.7 s / 2.1 s | 0.6 s / 6.2 s |
| Run: separate / merged | 12.0 s / 3.5 s | 68.9 s / 12.1 s |
| Result when merged | 201 pass | 708 pass, **14 fail** (the process-wide view-cache tests) |

### 2.3 Arithmetic behind "33 minutes to 7" (estimate)

Per batch today: 5.0 Swift checks × 216 s = 18 min; 1.7 real app builds × 185 s = 5 min; 1.46 suite
runs × 391 s = 9.5 min. After: five Debug builds at 14–23 s = 1.5 min; one Release build = 2.6 min; one
suite run (46 s build, about 100–130 s run four at a time) = 2.5–3 min. This does not count the model
turns spent polling, which also go away once the gate fits in one command.

## 3. Changes, in order

### 3.1 Swift commands (one small run)

- `Makefile` `typecheck`: add `-enable-batch-mode -j 12`. 203 s → 12 s.
- New `make app-debug`: `rust project`, then `xcodebuild -configuration Debug` into its own
  derived-data folder. This becomes the Swift check **and** the app agents drive in the loop: it is as
  fast as the typecheck, incremental, catches the SILGen diagnostics the typecheck cannot, and switches
  on the 58 `assert` sites Release compiles out (one is the catalog/handler parity check).
- `make app` (Release) runs once per batch, at the final gate. No typecheck before an app build.
- `project.yml`, "Build Rust Core" script: set `basedOnDependencyAnalysis: false`. A bare `xcodebuild`
  after a Rust edit skipped this script and would have linked a stale library; `make app` is only safe
  because it runs cargo first.
- Before adopting: run the existing MCP drives once against the Debug app. Known differences are the
  asserts, about 16 Swift files with whole-canvas loops that run unoptimised, and
  `scripts/verify_phase4.py`, which has wall-clock bounds and should stay on Release. 19 of the 20 probe
  runners already compile the app unoptimised.

### 3.2 One gate, run once (one run, plus edits to the litecode files)

- `scripts/gate.py` and `make gate`: reads the changed paths; runs lint, the test build and the Swift
  build side by side after one `cargo build`; runs tests with `--no-fail-fast`; takes a lock so two
  builds never overlap; prints one summary line per step and appends seconds and exit codes to an
  untracked `build/gate-times.tsv`.
- Whole suite **once per batch, by the implementer, when the code is complete, before review.** Not
  after review: across 221 batches the suite caught 5 real regressions that targeted tests had missed,
  and 4 of them were resolved by editing an existing test's assertion, which a reviewer should see.
- Fixers run targeted checks; one whole-suite run before the commit if a fix pass edited `core/`. In
  three sessions, 50 fixer re-runs of the suite caught nothing. Treat as a trial with a counter in the
  timing log.
- The scoping rule is coarse on purpose: 59% of commits touch both sides. Anything touching `ffi_*.rs`,
  the header, `RasterCore.swift`, manifests or build files counts as both; a Rust-only change still
  relinks the app when a drive follows.
- The orchestrator's own gate runs (27 typechecks, 11 app builds in the session) go through the same
  script.

### 3.3 Test run (same run as 3.2, or the next)

- Run test binaries about four at a time (the gate script can do this from
  `cargo test --no-run --message-format=json`, or cargo-nextest can): 497 s → roughly 100–130 s with no
  source change.
- A heavy tier for the large-document tests (`filtered_source_ops_24mp`,
  `filtered_interactive_ceilings`, `live_filter_45mp`, `live_filter_admission`, the 24 MP floors in
  `live_filter_rank_band`: 110–135 s). Run it at every batch's final gate, alone, not in mid-run suites.
- `make test` builds test targets only (`--tests`); later move the two in-library unit tests out and set
  `test = false`, `doctest = false`, which removes a second compile of the library.
- One-time experiment: add the terminal that runs the agents to Privacy & Security → Developer Tools and
  re-time a whole-suite run after a rebuild, to see whether the 135 s first-launch cost disappears.

### 3.4 Stop the growth

**Test binaries** (about 90 new ones a week).

- Change the rule in `core/CLAUDE.md`: a new feature area is a new test *file* declared in an area
  suite, never a new binary; a new binary needs a stated process-isolation reason. Add a guard test that
  fails when a `core/tests/*.rs` file is declared in no suite or in two.
- Merge in stages, without moving files: (1) suite roots that mount existing files with `#[path]`, which
  needs no test-file edit and is proven to build; (2) later, compile `tests/common` once per suite.
- Choose the files that must not share a process **by behaviour**: the 8 with their own global
  allocator, `process_working_limit`, `durable_save`, every file that calls the process-wide cache
  eviction or asserts on views, memo entries or `weak_count`, and the files with wall-clock assertions
  until those are converted to the park seam. The 14 prototype failures are this set.
- Acceptance is a keyed comparison of test lists before and after, plus `make lint`, not a count.
- A wrapper such as `make t F=<file> T=<filter>` so nobody needs to know which suite a file is in.

**Context.** `core/CLAUDE.md` (283 KB) and `app/CLAUDE.md` (222 KB) are injected whole, about 100k and
80k tokens, on the first tool call that touches the directory, and carried to the end of the run: about
26% of implementer input (43% for fixers). The load is also unreliable: 29 of 70 implementers were never
sent the core rules, 49 of 70 never the app rules, and 17 edited Swift without them. Keep the rules
(4.5 KB and 13.5 KB) in the `CLAUDE.md` files; move the per-file maps to `core/MODULES.md` and
`app/FILES.md`, one line per file, grepped and never read whole.

**Docs on every change.** README is edited by 89% of commits and a status doc by 92%; they cause most
simulated merge conflicts and 21 of 53 actionable review findings. Write a short change note per batch
and fold notes into README and status docs in a periodic sweep.

**Probe runners.** All 20 recompile the whole app into a fresh folder on every run (jobs-probe alone: 64
runs, 5.6 h). One shared helper with a persistent module cache and a build cached on a hash of its
inputs.

**Hub files.** Divide `RasterCore.swift` (7,889 lines, in 41–50% of Swift commits) along its 41 `MARK`
sections and `AgentCatalog.swift` along its 14 area functions; give the handler table, action steps,
precision whitelists and menus per-area tables. This is for smaller reads, smaller edits and fewer
collisions, not for build time.

**Rust settings.** A named loop profile separate from the shipping one (today the shipped library is
built with the loop's `incremental = true`, hence 256 codegen units); explicit `image` features (the
default set pulls in an AV1 encoder); a rule and lint that keep pixel kernels from importing the
document types; fix the `Cargo.toml` comment that cites a document from another repository.

## 4. On splitting into crates and modules

Splitting is standard practice in large Rust and Swift projects. The reasons their maintainers give are
organisation, parallel compilation of a cold build, and skipping code that does not depend on a change.
It does not shorten a rebuild after editing code that everything else depends on, because Cargo and the
Swift build system both rebuild every dependent of a changed unit. The same projects first separate a
fast loop configuration from the shipping one, keep test binaries few, and gate slow tests.

Here:

- **Rust.** A type's methods must live in the crate that defines the type. That pins 130 files (33%)
  directly and, with what they call, 79% of the crate at file level; about 30% is free of the document
  types if individual items are relocated. 70–84% of library rebuilds in the transcripts touch the pinned
  part. The whole library rebuilds in 12 s and builds cold in 46 s, so there is little for a split to
  win.
- **Swift.** In a six-module layering the median commit still rebuilds 97% of lines. In Debug the single
  module is already incremental, and a full recompile is 20 s, so a wrapper module to contain header
  edits has nothing to save.

Revisit when one of these is measured: a full Debug recompile over about a minute; a library rebuild
after an edit over about 30 s; the once-per-batch Release build (152 s, single-threaded, linear in code)
becoming the longest step of the gate; or kernel-only edits above a quarter of Rust builds with tests
allowed to link kernels directly.

## 5. Keeping it fast

The gate's timing log is the control. Proposed thresholds that trigger a look: Swift check over 30 s,
full gate over 8 minutes, whole-suite run over 3 minutes, a nested `CLAUDE.md` over 20 KB. Re-profile
after about 20 batches; the transcript scripts from this study can be re-run without building anything.

## 6. Not established

- Whether every existing drive passes against a Debug app (only a launch and `tools/list` were tried).
- The cause of the first-launch cost of new test binaries.
- Wall time of the suite four at a time when binaries are freshly linked (estimated 100–130 s).
- Merged-suite sizes beyond the two prototypes; peak memory when large tests share a process.
- A suspected flaky assertion (error text must not contain "png", while the text ends with a random
  temp directory name) did not reproduce with a temp path containing "png".
- Why fix rounds rose from 14% to 44% of batches.
- Effect of the `CLAUDE.md` split on wall-clock; only the token share is measured.
