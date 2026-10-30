# Build velocity: status

Working record of `designs/build-velocity-plan.md`; the evidence behind the plan is in `designs/build-velocity-proposal.md`. One table per phase, each number with the command that produced it. Phases 1–4 as first recorded (every timing row, the Debug-settings variants, the script output and the Phase 3 per-drive table) are in [archive/2026-10/build-velocity-status.md](../archive/2026-10/build-velocity-status.md). Timings are wall time of the whole command, one run each, on an Apple M2 Max (12 logical CPUs) with Xcode 26.2.

## Shipped runs

Phases 1 to 9a and 9c are in [build-velocity-shipped.md](build-velocity-shipped.md).

## Phase 9b

Status docs: a rule, a check and the clean-up (plan step 9b). Runs, in order:

- **part 1** (done): the "Status docs" rule in the root `CLAUDE.md`; `scripts/check_docs.py` as the `docs` step of `make gate`; `scripts/verify_status_condense.py`, which proves a condense pass; the four status docs over 40 KB and this doc condensed, with their originals in `archive/2026-10/`.
- **part 1b** (done): the nine shipped-phase ADRs that grew by more than 2 KB since September, condensed back to the rule (a closed issue is a few lines, not its record), with their originals in `archive/2026-10/`, largest first: `phase-11c-output-color-status.md`, `phase-11d-format-additions-status.md`, `phase-11f-hdr-float-status.md`, `phase-12d-comparison-views-status.md`, `phase-10c-constructs-status.md`, `phase-8g-raw-enhancements-status.md`, `phase-12b-actions-status.md`, `phase-12c-workspace-status.md`, `phase-12a-replay-status.md`.
- **part 2** (done): `phase-13a-regional-status.md` condensed as the working record of an open phase: 13A-latency's contract in full (its plan row, the measured regression, its open items), the 31 shipped sub-slices cut to a few lines each with their commits, the decisions that code does not state, and every standing open item as a tagged bullet; the original is in `archive/2026-10/`, and the doc's allowance entry is removed from `check_docs.py`.
- **part 3** (done): `phase-13b-tiled-status.md` condensed as the working record of an open phase: the six runs not shipped (13B-4 … 13B-9) with their plan rows verbatim and an issue each, the table of 16 items handed from 13A unchanged, the 30 shipped runs cut to a few lines each with their commits, the decisions that code does not state, and every standing open item as a tagged bullet; the original is in `archive/2026-10/`, and the allowance mechanism is removed from `check_docs.py`, so the 40 KB cap has no exception.
- **part 3b** (done): `phase-13a-regional-status.md` and `phase-13b-tiled-status.md` each split into the working record and a shipped companion (`phase-13a-regional-shipped.md`, 63 sections; `phase-13b-tiled-shipped.md`, 90 sections) holding the contracts, decisions and verification records of the shipped runs, moved whole and byte for byte, with a one-line `## Shipped runs` section in their place. `python3 scripts/verify_status_split.py --base f182a99 phase-13b-tiled-status.md phase-13a-regional-status.md` proves it: 12 `ok` lines, exit 0. The "Status docs" rule names the companion, `check_docs.py` caps `designs/*-shipped.md` at 32 KB, and the one citation of a moved section (`core/MODULES.md`'s `precision_image.rs` entry) is repointed to the companion.

| Doc, bytes | Command (Before is the same on `archive/2026-10/`) | Before | After |
|---|---|---|---|
| `phase-9f-selected-tone-status.md` | `wc -c designs/phase-9f-selected-tone-status.md` | 98320 | 29456 |
| `phase-4-jobs-resource-status.md` | `wc -c designs/phase-4-jobs-resource-status.md` | 68911 | 25786 |
| `phase-9-live-filters-status.md` | `wc -c designs/phase-9-live-filters-status.md` | 49216 | 28109 |
| `phase-12g-clipboard-status.md` | `wc -c designs/phase-12g-clipboard-status.md` | 44081 | 31141 |
| `phase-11c-output-color-status.md` | `wc -c designs/phase-11c-output-color-status.md` | 23930 | 21718 |
| `phase-11d-format-additions-status.md` | `wc -c designs/phase-11d-format-additions-status.md` | 20797 | 16614 |
| `phase-11f-hdr-float-status.md` | `wc -c designs/phase-11f-hdr-float-status.md` | 16779 | 15703 |
| `phase-12d-comparison-views-status.md` | `wc -c designs/phase-12d-comparison-views-status.md` | 15339 | 15175 |
| `phase-10c-constructs-status.md` | `wc -c designs/phase-10c-constructs-status.md` | 14794 | 13608 |
| `phase-8g-raw-enhancements-status.md` | `wc -c designs/phase-8g-raw-enhancements-status.md` | 13796 | 8499 |
| `phase-12b-actions-status.md` | `wc -c designs/phase-12b-actions-status.md` | 13583 | 13012 |
| `phase-12c-workspace-status.md` | `wc -c designs/phase-12c-workspace-status.md` | 12412 | 12266 |
| `phase-12a-replay-status.md` | `wc -c designs/phase-12a-replay-status.md` | 8447 | 8519 |
| `phase-13a-regional-status.md` | `wc -c designs/phase-13a-regional-status.md` | 486083 | 37904 |
| `phase-13b-tiled-status.md` | `wc -c designs/phase-13b-tiled-status.md` | 540587 | 39420 |
| `phase-13a-regional-status.md`, part 3b | `wc -c designs/phase-13a-regional-status.md` (Before: `git show f182a99:designs/phase-13a-regional-status.md \| wc -c`) | 37904 | 14636 |
| `phase-13a-regional-shipped.md` | `wc -c designs/phase-13a-regional-shipped.md` | none | 23899 |
| `phase-13b-tiled-status.md`, part 3b | `wc -c designs/phase-13b-tiled-status.md` (Before: `git show f182a99:designs/phase-13b-tiled-status.md \| wc -c`) | 39420 | 20211 |
| `phase-13b-tiled-shipped.md` | `wc -c designs/phase-13b-tiled-shipped.md` | none | 19845 |
| `build-velocity-status.md` | `git show 1e2f1bd:designs/build-velocity-status.md \| wc -c` | 29972 | 10487 at part 1 |

- **The gate.** `make gate` after the `gate.py` edit ran both legs: `gate: PASS 222.9s`, with `docs  exit=0  0.0s` (fmt 6.4, clippy 0.7, tests 215.5, app 2.4, launch 0.5).
- **A deliberate oversize.** With 45,000 bytes appended to `designs/phase-1-integrity-status.md`, `make gate` returned status 2 and `gate: FAIL (docs) 0.3s`, printing `error: designs/phase-1-integrity-status.md: the status doc is over the cap (49947 > 40960)`; every step but `wait`, `docs` and `core-build` was skipped. After `git checkout`, `gate: PASS 0.3s`.
- **Sizes.** The four phase records are over the run's 24,000-byte target and under the 40 KB cap: each keeps decisions in force that code and tests cite by name. Of part 1b's nine records one met its target (`phase-8g-raw-enhancements-status.md`); the other eight are over theirs by 466 to 3,114 bytes for the same reason, and the largest is 21,718 bytes. `phase-12a-replay-status.md` ends larger than its original: it gained an open item and the pointer to its archived original. Part 2's 13A record is 37,655 bytes against its 36,000-byte target: over by 1,655 bytes and under the cap; what does not fit in the target is two kept decisions that code does not state and the five open-item bullets for residue the original's table did not list. Part 3's 13B record is 39,420 bytes against its 36,000-byte target, over by 3,420 bytes and 1,540 under the cap (what does not fit in the target is the 15 open-item bullets owned by 13A-latency, each kept with its measured line, bound and cause, and the plan and admission clauses its sections handed to 13B-8), and the 13A record is now 37,904 bytes, because part 3 appended the three issue links of the runs that own its `TRACKED` items.
- **Open items.** Four issues were filed from the 9F record and two from the Phase 9 record; every other open item in the five docs already had an issue. Part 1b filed four more: two from the 11D record, one from 12B and one from 12C. Part 2 filed three from the 13A record: one for 13A-latency, one for the seven items owned by 13D, and one for three leftovers its open-items table did not list. Part 3 filed eight from the 13B record: one per run not shipped (13B-4 to 13B-9), one for the item owned by 13C, and one for three leftovers with no owner; the 15 rows owned by 13A-latency went to the 13A-latency issue as a comment.

## Phase 10

Shared test modules compiled once per suite. Each of the 8 suite roots (`core/tests/suite_*.rs`) now declares `mod common;`, `mod live_filter_support;` where its files use it, and one `#[path = "common/<helper>.rs"]` module per helper its files mount; the mounted files reach them with `use crate::…;`. Of the 180 `mod common;` lines in mounted files, 177 became `use crate::common;`, 2 were dropped (`encode_refusals.rs`, `psd_tests.rs` never named `common`), and `precision_scalar_statistics.rs`'s (a `native_scalar` helper mount) became `use crate::native_scalar as common;`. The 154 helper mounts in 115 mounted files and the 15 `live_filter_support` declarations moved to the roots; `sha256` is reached as `crate::common::sha256`. The 21 standalone binaries are unchanged. `test_layout::no_mounted_file_declares_a_shared_module` fails on a mounted file that declares `mod common;` or `mod live_filter_support;` or mounts a `common/` helper itself.

| Measurement | Command | Before | After |
|---|---|---|---|
| Suites after a `tests/common` edit (s) | `touch core/tests/common/mod.rs; cargo test --release --no-run --test 'suite_*'` | 3.52 | 2.77 |
| All binaries after a `tests/common` edit (s) | `touch core/tests/common/mod.rs; cargo test --release --no-run` | 4.96 | 4.48 |
| Suites after a `core/src` edit (s) | probe static in `durable_file.rs`; `cargo test --release --no-run --test 'suite_*'` | 13.13 | 11.10 |
| All binaries after a `core/src` edit (s) | probe static changed; `cargo test --release --no-run` | 22.88 | 14.63 |
| Gate after a `core/src` edit | `make gate` | `gate: PASS 302.4s`, tests 274.2 | `gate: PASS 286.7s`, tests 270.0 |
| Keyed test list | `python3 scripts/verify_test_merge.py --compare … --new-module test_layout` | 3936 tests, 44 ignored | 3937 tests, 44 ignored (the guard added) |

- **The guard bites.** With `core/tests/adjust_levels.rs` reverted to `mod common;`, `make t F=test_layout` failed: `` tests/adjust_levels.rs (mounted by suite_style_adjust) declares its own `mod common;`: the suite root declares it once; use `use crate::common;` ``.
- **The 300 s gate.** The gate after a `core/src` edit is now under 300 s (286.7 s against 302.4 s), but the tests step moved only 4.2 s (274.2 to 270.0): almost all of it is the test run, not the build. The item stays open.
- **Timings** are one run each of incremental release builds after an untimed warm `cargo test --release --no-run`; the logs and `timings.tsv` are in the run's handoff directory. No issue was filed.

One serial suite for the cache-state test files. The 29 standalone binaries isolated only because they evict or assert on the process-wide view cache or memos are now files mounted in `core/tests/suite_serial.rs`. The root declares the shared modules once and one `static SERIAL` lock; every test in a mounted file takes it with `let _serial = serial();` (`use crate::serial;`) as its first statement, so its guard drops last and no two of these tests run at once under either `make test` or plain `cargo test`. The 23 file-local locks were deleted (`resources.rs`'s `test_lock()` became `serial()`), the lock was added to every test of `doc_tests`, `independent_masks`, `layer_raster`, `native_reopen_plan8`, `native_save_plan8` and `precision_content_box`, and `layer_raster_writers.rs`'s lock moved out of the `run_site` helper into its 9 callers. `scripts/run_tests.py` runs `suite_serial` in a lane of its own beside the pool and the other suites and joins it before the solo list. `live_filter_native` and `style_cache` stay standalone until a later run converts their timing. `test_layout::every_serial_suite_test_takes_the_lock_first` fails on a test in a file `suite_serial` mounts whose first statement is not the lock.

| Measurement | Command | Before | After |
|---|---|---|---|
| Test executables | `python3 scripts/verify_test_merge.py --write …` | 92 | 64 |
| Whole suite, warm (s) | `python3 scripts/run_tests.py --summary …` (`seconds`) | 241.5 | 229.6 |
| Gate after a `core/src` edit | probe static in `durable_file.rs`; `make gate` | `gate: PASS 310.2s`, tests 288.0 | `gate: PASS 272.9s`, tests 256.2 |
| Keyed test list | `python3 scripts/verify_test_merge.py --compare … --new-module test_layout` | 3937 tests, 44 ignored | 3938 tests, 44 ignored (the guard added) |

- **The guard bites.** With the lock line deleted from `native_save_plan8.rs`'s first test, `make t F=test_layout` failed: ``tests/native_save_plan8.rs:343 (mounted by suite_serial): the test's first statement is not `let _serial = serial();`: every test in this suite takes the shared lock first, so it drops last``.
- **The 300 s gate.** The gate after a `core/src` edit is under 300 s again (272.9 s against 310.2 s), and the tests step moved 31.8 s (288.0 to 256.2). `suite_serial` takes 47.3 s in its own lane. The item stays open.
- **Two files**, `live_filter_native` and `style_cache`, join `suite_serial` once their timing is converted. No issue was filed.

Default cancel-latency tests park; their bounds move to `make cancel-bench` (run 1 of 2). The 21 standalone files whose default tests asserted a cancel-to-return bound now prove the cancel with the park seam: `common::probe::entry_then_cancel` takes the entry report (and the next one) from a twin run's own report log, and `cancelled_parked_at` (or the seam directly, where the error is not exactly `operation cancelled` or the control is reused) cancels with the worker parked at `n0 + 1 + k`. Each original timed body stands unchanged in the file's `mod bench` twin, `#[ignore = "make cancel-bench: a wall-clock verdict"]`, under the same name, and the 21 stems joined `CANCEL_BENCH_FILES`. The 13 `precision_*` files are mounted in `suite_precision` (which now also declares `precision_native_fixture`) and the other 8 in `suite_retouch`; their `[[test]]` entries are gone. `test_layout::no_default_test_asserts_a_duration` fails on an assertion over a clock reading (`Instant`, `Duration`, `.elapsed()`, `as_secs`…, or a name bound from one) outside an `#[ignore]` fn or `mod bench`; the 10 files run 2 converts are exempt by name in `PENDING_516` (`content_aware_cost_tests`, `content_aware_tests`, `heal_tests`, `job_control_park`, `live_filter_native`, `lut_tests`, `metadata_packet_framing`, `metadata_tests`, `raw_source_geometry_safety`, `style_cache`).

| Measurement | Command | Before | After |
|---|---|---|---|
| Test executables | `python3 scripts/verify_test_merge.py --write …` | 64 | 43 |
| Keyed test list | `python3 scripts/verify_test_merge.py --compare … --new-module test_layout --new-module <file>::bench …` | 3938 tests, 44 ignored | 3964 tests, 69 ignored (the guard and 25 `::bench::` twins added) |

- **The guard bites.** With `assert!(start.elapsed() < Duration::from_millis(500));` re-added to `precision_prototype.rs`'s default cancel test, `make t F=test_layout` failed: `tests/precision_prototype.rs:492: an assertion on a duration in default-run code: …`. With `lut_tests` taken out of `PENDING_516` it failed on `tests/lut_tests.rs:1024`, the `distinct_secs < repeated_secs * 8.0 + 0.05` assertion over two names bound from a timing closure. With `PENDING_516` emptied it flagged all 10 pending files and no other.
- **Timings** for the 300 s gate are taken by run 2; the before figures are in the run's handoff `timings.tsv`. No issue was filed.

The last default-run timings become twins or parks; `PENDING_516` is gone (run 2 of 2). Seven performance-ratio files keep every functional assertion (refusal messages, fills, pixels, per-open checks, cache `retained` / `fresh_render` checks) in the default test. Each ratio or bound moves into a verbatim `mod bench` twin, 15 twins in all. `content_aware_cost_tests`, `content_aware_tests` and `heal_tests` are mounted in `suite_retouch` and are out of `SOLO`. `lut_tests` is in `suite_style_adjust`, `metadata_packet_framing` and `metadata_tests` are in `suite_color_formats`, and `style_cache` is in `suite_serial`, with the lock first in every test and twin. `heal_tests::measured_cost_of_a_million_unknowns` is ignored in place. The seven stems joined `CANCEL_BENCH_FILES`. Twelve poller-race tests in eleven files now cancel with the worker parked. They are in `linked_source_ops`, `persisted_warp_safety` (each depth), `phase8e_coverage` and `precision_layer_blending` (rung 0 only), `precision_codec`, `raw_local_native`, `refinement_execution` (2), `selected_adjustments`, `raw_source_geometry_safety`, `live_filter_native` (in `suite_serial`; it clears while parked) and `develop_inspection`. Two parks are reservation parks, on the first checkpoint after `reserve` stores. Four parks fall back to k = 0, because their kernel checks only at its reports: the two `refinement_execution` tests, `precision_layer_blending`, and `develop_inspection`'s histogram case. `job_control_park` asserts a deadline hang guard (`Instant::now() < deadline`). The 18 `[[test]]` entries are deleted. `test_layout::no_default_test_asserts_a_duration` now scans every file.

| Measurement | Command | Before | After |
|---|---|---|---|
| Test executables | `python3 scripts/verify_test_merge.py --write …` | 43 | 25 |
| Keyed test list | `python3 scripts/verify_test_merge.py --compare … --new-module <file>::bench …` | 3964 tests, 69 ignored | 3979 tests, 85 ignored (15 `::bench::` twins; one `state differs`, the million-unknowns cost now ignored) |
| Whole suite, warm (s) | `python3 scripts/run_tests.py --summary …` (`seconds`) | 227.0 | 240.3 |
| Gate after a `core/src` edit | probe static in `durable_file.rs`; `make gate` | `gate: PASS 283.0s`, tests 260.4 | `gate: PASS 280.1s`, tests 256.7 |

- **The guard bites.** With `let one = std::time::Instant::now().elapsed();` and `assert!(one < one * 4);` added to `metadata_packet_framing.rs`'s default test, `make t F=test_layout` failed: `tests/metadata_packet_framing.rs:307: an assertion on a duration in default-run code: …`. The `assert!` line names no clock; the guard reaches it through the tainted `one`.
- **The 300 s gate is met.** The gate after a `core/src` edit is 280.1 s, under the 300 s target; it was 283.0 s before run 1. The whole-suite figure is one run each, and an outside build may have overlapped the timed after run; the untimed warm-up pass of the same command took 218.4 s.
- **Left open.** The report-log PIN ratio `common::probe::assert_pin` is a wall-clock ratio that the guard does not scan, because `common/` is exempt. The regional default tests still race a poller against `!worker.is_finished()`.

**Phase 10 done 2026-10-08.** Its three items shipped. The plan's fourth bullet, the re-profile after about 20 batches, was filed as its own issue.
