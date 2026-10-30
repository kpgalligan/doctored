# Build velocity: status

Timings for each phase of `designs/build-velocity-plan.md`; the evidence behind the plan is in `designs/build-velocity-proposal.md`.

## Phase 1

Measured 2026-10-06 on an Apple M2 Max (12 logical CPUs), Xcode 26.2 (17C52), rustc 1.92.0, 655 Swift files in `app/Sources`. Wall time of the whole command, one run each. "Before" is the proposal's number for the command the loop used until now (`make typecheck` unbatched; `make app`, Release). Every timed `make app-debug` followed an untimed build of the preceding state, and every edit was reverted.

| Measurement | Command | Before | After | Expected |
|---|---|---|---|---|
| Type check, clean tree | `make typecheck` | 203 s | 12.6 s, exit 0 | 12 s |
| Type check, one injected type error in `app/Sources/CropTool.swift` | `make typecheck` | 145 s | 12.6 s, exit 2, error printed | 12 s |
| App build, cold (no `build/Build`) | `make app-debug` | 171 s (Release) | 62.1 s, of which cargo rebuilt the core in 23.1 s; 39 s without it | 33 s |
| App build, no change | `make app-debug` | 1.3 s (Release) | 1.8 s | 2 s |
| App build, one statement changed in `app/Sources/CropTool.swift` | `make app-debug` | 156 s (Release) | 14.3 s | 14 s |
| App build, top-level function added to `app/Sources/RasterCore.swift` | `make app-debug` | — | 21.9 s | 23 s |
| App build, comment added to `core/include/rasterize_core.h` | `make app-debug` | — | 20.6 s | 20 s |
| App build, one injected type error in `app/Sources/CropTool.swift` | `make app-debug` | — | 4.2 s, exit 2, error printed | 17 s |

The cold build was the first build of the worktree: `core/target/release/librasterize_core.a` was older than the core sources, so `make`'s `rust` step compiled the crate (23.1 s) before Xcode started. The cargo run inside Xcode then took 0.13 s. It was also measured before the Debug settings below were chosen, so it ran with neither; every other `make app-debug` row ran with both.

**Debug settings (plan item 4).** Each variant: `make app-debug DEBUG_XCFLAGS=<setting>`, two successive one-statement edits in `app/Sources/CropTool.swift`, the Debug app then launched under the background recipe and asked `tools/list`.

| Setting | First edit | Second edit | Best | Launches, answers `tools/list` |
|---|---|---|---|---|
| none | 14.7 s | 14.2 s | 14.2 s | yes (269 tools) |
| `COMPILER_INDEX_STORE_ENABLE=NO` | 20.5 s | 13.2 s | 13.2 s | yes (269 tools) |
| `ENABLE_DEBUG_DYLIB=NO` | 23.7 s | 13.5 s | 13.5 s | yes (269 tools) |

Decision: both settings are kept (`DEBUG_XCFLAGS := COMPILER_INDEX_STORE_ENABLE=NO ENABLE_DEBUG_DYLIB=NO`); neither best time is above the no-settings best. The differences (0.7–1.0 s on one run each) are within what one run can show, so this records "not slower", not a measured gain. A changed setting does nothing on a build with no source change (2.1 s and 2.3 s); the first build that compiles after it recompiles every file, which is the 20.5 s and 23.7 s in the "First edit" column. With `ENABLE_DEBUG_DYLIB=NO` the product has no `Rasterize.debug.dylib`: `Contents/MacOS/Rasterize` is the whole binary.

**Cargo on every build (plan item 3).** Around the "no change" build, `stat -f %m core/target/release/librasterize_core.a` gave 1791296258 before and 1791296258 after: cargo ran twice (once from `make`, once from Xcode's script, 0.05 s each) and rebuilt nothing in either environment.

**Bare `xcodebuild` after a Rust edit.** A comment was appended to `core/src/durable_file.rs` (mtime 1791296726), then `xcodebuild -project Rasterize.xcodeproj -scheme Rasterize -configuration Debug -derivedDataPath build COMPILER_INDEX_STORE_ENABLE=NO ENABLE_DEBUG_DYLIB=NO build` was run directly, not through `make`: 6.3 s, cargo compiled the crate (3.2 s) and the app relinked. Afterwards `librasterize_core.a` was 1791296732 and `Contents/MacOS/Rasterize` 1791296733, both newer than the edit (before it: 1791296258 and 1791296715).

**Stale refusal.** With `app/Sources/CropTool.swift` touched after the last Debug build, `scripts/verify_phase13a6a_bounded_selection.py` exited 1 with "refusing to launch …/Debug/Rasterize.app/Contents/MacOS/Rasterize: older than app/Sources/CropTool.swift. Rebuild with `make app-debug`, or set RZ_ALLOW_STALE_APP=1 to launch it anyway." on stderr, and `scripts/verify_pixel_art_tools.py` exited 1 with the same sentence in its JSON `error` field; neither wrote an `app.log`, so nothing was launched. With the Debug app still stale and `RZ_VERIFY_APP=build/Build/Products/Release/Rasterize.app/Contents/MacOS/Rasterize`, `verify_phase1.instance` launched the Release app built by `make app` (172.9 s) and `tools/list` answered 269 tools.

## Phase 2

Measured 2026-10-06 in the worktree, against base commit `8384ac9`. `core/CLAUDE.md` and `app/CLAUDE.md` keep their rules; every per-file entry moved to `core/MODULES.md` and `app/FILES.md`, which are grepped by file name and never read whole. The root `CLAUDE.md` imports the two rule files.

| Measurement | Command | Before | After | Limit |
|---|---|---|---|---|
| Root `CLAUDE.md`, bytes | `wc -c CLAUDE.md` | 10127 | 10182 | — |
| `core/CLAUDE.md`, bytes | `wc -c core/CLAUDE.md` | 282663 | 4856 | under 8000 |
| `app/CLAUDE.md`, bytes | `wc -c app/CLAUDE.md` | 222189 | 14809 | under 16000 |
| `core/MODULES.md`, bytes | `wc -c core/MODULES.md` | no file | 278278 | — |
| `app/FILES.md`, bytes | `wc -c app/FILES.md` | no file | 207988 | — |
| Core entries | `python3 scripts/verify_claude_md_split.py --base 8384ac9` | 161 in `core/CLAUDE.md` | 161 in `core/MODULES.md`, none in `core/CLAUDE.md` | equal, byte for byte |
| App entries | `python3 scripts/verify_claude_md_split.py --base 8384ac9` | 130 in `app/CLAUDE.md` | 130 in `app/FILES.md`, none in `app/CLAUDE.md` | equal, byte for byte |

**Verbatim move.** `python3 scripts/verify_claude_md_split.py --base 8384ac9` exited 0. An entry is a line that starts with `- `, plus the one `app/CLAUDE.md` line that starts with `` `PersistedWarp.swift` / `` and never had a bullet. No entry was shortened, reordered, merged or rewrapped: 156 core and 128 app entries are longer than the 200 characters the index headers allow a new entry. The script's output:

```
ok core check 1: core/MODULES.md holds 161 entries byte for byte, in order, after a 3-line header
ok core check 2: core/CLAUDE.md keeps 10 non-blank lines in order; 1 line is new (line 5): b"The per-module index is `core/MODULES.md`: grep it by file name before editing or adding a module (`grep -n -F 'doc_sele"
ok core check 3: no line of core/CLAUDE.md starts with '- ' (21 lines)
ok core check 4: core/CLAUDE.md is 4856 bytes (limit 8000)
ok app check 1: app/FILES.md holds 130 entries byte for byte, in order, after a 3-line header
ok app check 2: app/CLAUDE.md keeps 28 non-blank lines in order; 1 line is new (line 5): b"The per-file index is `app/FILES.md`: grep it by file name before editing or adding a file (`grep -n -F 'Bitmap.swift' a"
ok app check 3: no line of app/CLAUDE.md starts with '- ' (43 lines)
ok app check 4: app/CLAUDE.md is 14809 bytes (limit 16000)
```

**Transcript check.** The root file's two `@` imports put both rule files in a subagent's first turn, so the import form of the root file shipped (the spec's variant A). The probe was one headless session started with the worktree as its working directory, Claude Code 2.1.291: `claude -p --output-format json 'Start exactly one general-purpose subagent whose whole prompt is: Reply with the single word ok and use no tools. When it returns, reply with the single word done.'` exited 0 with result `done` and wrote one subagent transcript, a `subagents/agent-*.jsonl` file under that session's folder in Claude Code's projects directory (`T` below). `jq -r 'select(.type=="attachment" and .attachment.type=="instructions") | .attachment.files[] | [.path, (.content|length), (.content|contains("**Modules and size.**")), (.content|contains("**Placement.**"))] | @tsv' "$T"` printed the path, the content length in characters, and whether the content holds the first core rule and the first app rule:

```
CLAUDE.md	10145	false	false
core/CLAUDE.md	4827	true	false
app/CLAUDE.md	14750	false	true
<Claude config dir>/projects/<project>/memory/MEMORY.md	2521	false	false
```

`jq -r '.type + ":" + (.attachment.type // "")' "$T" | grep -n -E '^(assistant:|attachment:instructions)' | head -2` printed `7:attachment:instructions` and `13:assistant:`, so the instruction files are record 7 and the first assistant record is 13. `jq -c 'select(.type=="assistant") | .message.usage | {input_tokens, cache_creation_input_tokens, cache_read_input_tokens}' "$T" | head -1` printed `{"input_tokens":2,"cache_creation_input_tokens":29157,"cache_read_input_tokens":0}`: the subagent's first turn read 29159 tokens in all. Neither index file is among the loaded instruction files.

**References.** Nine Swift comments cited `app/CLAUDE.md` for text that now lives in a moved entry, and each now names `app/FILES.md`, one token on one line: `Guides.swift` 88, `LayersPanelViewController+Rows.swift` 9 and `LayersPanelViewController.swift` 19 (the frozen-four entry); `RawImage.swift` 326 (the `Bitmap.swift` entry); `ActionsViewController.swift` 761, `EditorViewController+Guides.swift` 57, `NewGuideSheetController.swift` 13 and `LayerSelection.swift` 15 (the "Shared chrome" entry); `SoftProof.swift` 278 (the `BackgroundWork.swift` entry). Fourteen citations in `app/Sources`, `core/src`, `core/tests` and `scripts` point at rules that stayed, and are unchanged; so are the recipe's citation in the root `CLAUDE.md` and those in `designs/phase-12c-workspace-status.md` and `designs/working-limit-ram-scaled.md`. The dated slice records in `designs/phase-13a-regional-status.md` and `designs/phase-13b-tiled-status.md`, and everything under `archive/`, record which file a past slice edited and are not rewritten.

**Verification skipped.** `make typecheck` ran and exited 0 with no error lines. `make test`, `make lint` and `make cancel-bench` did not run: no Rust source or test changed. `make app-debug`, `make app` and every app drive did not run: the Swift edits are nine comment tokens, which the type check covers, and the new `.md` files reach no build.

## Phase 3

Measured 2026-10-06 on the same Apple M2 Max (12 logical CPUs, 96 GB), macOS 26.5.1, Xcode 26.2 (17C52), in the worktree at commit `6630ce3`. Both apps were built from that tree before the first drive (`make app-debug`, then `make app`) and not rebuilt. Each of the 100 in-scope `scripts/` drives ran against the Debug app, then the Release app, as `python3 scripts/<drive> <arguments>`, serially, one app instance at a time, under a 1200 s limit per run. Times are wall time of the whole command, one run each; every failing run was run a second time on the same configuration, and its row shows the second run. The first pass took 4238 s of its 14400 s budget, so no drive was cut. Rows are in the order run: the 19 drives used in handoffs since 2026-10-01, then the other 81, newest phase first.

Arguments. `--app` (or `RZ_VERIFY_APP`, for the twelve drives that read only the environment) is the executable `…/Contents/MacOS/Rasterize`, and the bundle for `verify_phase6c.py`. The environment adds `RZ_BACKGROUND=1`, `ANTHROPIC_API_KEY=dummy`, `RZ_WORKING_LIMIT_BYTES=2147483648` and a private `TMPDIR`. `--output`, `--receipts` and `--report` are fresh paths outside the repository; `--keep` and `--require-current` are passed where a drive has them (not `--require-current` to `verify_phase8g.py`, which then requires groups only its host half produces). Drives with a host half that compiles its own app ran the app half only: `--mcp-only` (8C, 8D) and `--mode mcp` (8F, 8G, 9A). Nothing launched Photoshop, GIMP or a network fetch: `verify_phase10_receipts.py --receipts R1,R2,R3` (the drive has no R4), `verify_phase12e3c_psd.py --skip-photoshop`, `verify_phase9c_quality.py --offline`. Fixtures for 8G, 9B and 9D were written once by their Python generators and shared by both configurations.

| Drive | Debug | Release | Debug ÷ Release | Outcome |
|---|---|---|---|---|
| `scripts/verify_phase12g_clipboard.py` | timeout 1216 s | pass 1114 s | > 1.1 | Release-only: exceeds the 1200 s run limit on Debug |
| `scripts/host-still-admission/verify.py` | pass 62 s | pass 57 s | 1.1 | ok |
| `scripts/verify_phase8c.py` | fail 4 s | fail 4 s | n/a | fails on both |
| `scripts/verify_phase8d.py` | pass 8 s | pass 7 s | 1.1 | ok |
| `scripts/verify_phase5.py` | pass 46 s | pass 45 s | 1.0 | ok |
| `scripts/verify_phase8g.py` | pass 87 s | pass 79 s | 1.1 | ok |
| `scripts/verify_phase11d_formats.py` | fail 13 s | fail 12 s | n/a | fails on both |
| `scripts/verify_phase11f_float.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_pixel_art_tools.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_issue32_proof_presets.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_issue129_duplicate_to_new_document.py` | pass 4 s | pass 4 s | 1.1 | ok |
| `scripts/verify_phase12f3_warp.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase12e1_structure.py` | pass 15 s | pass 13 s | 1.2 | ok |
| `scripts/verify_phase12b2_presets.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase12a_replay.py` | fail 6 s | fail 6 s | n/a | fails on both |
| `scripts/verify_phase12h_conversion.py` | pass 8 s | pass 8 s | 1.1 | ok |
| `scripts/verify_phase12f2_geometry.py` | pass 5 s | pass 5 s | 1.0 | ok |
| `scripts/verify_phase12f1b3_rotation.py` | pass 10 s | pass 10 s | 1.0 | ok |
| `scripts/verify_phase12f1b_geometry.py` | pass 10 s | pass 10 s | 1.1 | ok |
| `scripts/verify_export_depth.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase13a1_regional.py` | pass 1 s | pass 1 s | 1.0 | ok |
| `scripts/verify_phase13a2_masks_clips.py` | pass 3 s | pass 3 s | 1.0 | ok |
| `scripts/verify_phase13a2b_byte_styles.py` | pass 1 s | pass 1 s | 1.0 | ok |
| `scripts/verify_phase13a3_groups.py` | pass 4 s | pass 3 s | 1.1 | ok |
| `scripts/verify_phase13a3b_styles.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase13a3bii_styled_groups.py` | pass 3 s | pass 3 s | 1.0 | ok |
| `scripts/verify_phase13a3c_knockout.py` | pass 3 s | pass 3 s | 1.1 | ok |
| `scripts/verify_phase13a4_guide.py` | pass 8 s | pass 8 s | 1.1 | ok |
| `scripts/verify_phase13a4b_nested_guide.py` | pass 10 s | pass 10 s | 1.1 | ok |
| `scripts/verify_phase13a4bii_pass_through.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase13a4biib2_styled_members.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase13a4biib_clip_members.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase13a6a_bounded_selection.py` | pass 8 s | pass 5 s | 1.6 | ok |
| `scripts/verify_phase13a6b_bounded_artwork.py` | pass 6 s | pass 6 s | 1.0 | ok |
| `scripts/verify_phase13a6c2_stroke_refusals.py` | pass 4 s | pass 3 s | 1.1 | ok |
| `scripts/verify_phase13a6c_bounded_strokes.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase12b_actions.py` | pass 47 s | pass 31 s | 1.5 | ok |
| `scripts/verify_phase12c2_workspace.py` | pass 1 s | pass 1 s | 1.2 | ok |
| `scripts/verify_phase12c_workspace.py` | pass 8 s | pass 7 s | 1.1 | ok |
| `scripts/verify_phase12d1_comps.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase12d2_views.py` | pass 5 s | pass 4 s | 1.3 | ok |
| `scripts/verify_phase12e1b_structure.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase12e1c_guides.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase12e2_adjustments.py` | pass 5 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase12e2b_placement.py` | pass 38 s | pass 37 s | 1.0 | ok |
| `scripts/verify_phase12e2c_stamp_flatten.py` | pass 12 s | pass 11 s | 1.1 | ok |
| `scripts/verify_phase12e3_knockout.py` | pass 10 s | pass 8 s | 1.2 | ok |
| `scripts/verify_phase12e3b_typed_knockout.py` | pass 23 s | pass 20 s | 1.2 | ok |
| `scripts/verify_phase12e3c_psd.py` (`--skip-photoshop`) | pass 13 s | pass 12 s | 1.1 | ok |
| `scripts/verify_phase12f1_geometry.py` | fail 11 s | fail 10 s | n/a | fails on both |
| `scripts/verify_phase12f5_paint.py` | pass 7 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase11a_export.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase11b_production.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase11c_softproof16.py` | pass 5 s | pass 5 s | 1.0 | ok |
| `scripts/verify_phase11e_linear.py` | fail 2 s | fail 2 s | n/a | fails on both |
| `scripts/verify_phase10_receipts.py` (R1–R3; R5–R9 launch GIMP) | pass 3 s | pass 3 s | 1.0 | ok |
| `scripts/verify_phase10a_external.py` | pass 6 s | pass 6 s | 1.0 | ok |
| `scripts/verify_phase10a_groups.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures come from `cargo test` |
| `scripts/verify_phase10a_markers.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures come from `cargo test` |
| `scripts/verify_phase10a_mask_foundation.py` | pass 9 s | pass 9 s | 1.0 | ok |
| `scripts/verify_phase10a_masks.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures come from `cargo test` |
| `scripts/verify_phase10a_pixels.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures come from `cargo test` |
| `scripts/verify_phase10a_relative_masks.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures come from `cargo test` |
| `scripts/verify_phase10b_external.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase10d_adjustment_blending.py` | pass 7 s | pass 6 s | 1.0 | ok |
| `scripts/verify_phase10d_blend_if.py` | pass 2 s | pass 2 s | 1.0 | ok |
| `scripts/verify_phase10d_blending.py` | fail 2 s | fail 2 s | n/a | fails on both |
| `scripts/verify_phase10d_five_modes.py` | fail 1 s | fail 1 s | n/a | fails on both |
| `scripts/verify_phase10d_four_modes.py` | pass 8 s | pass 8 s | 1.0 | ok |
| `scripts/verify_phase10d_hsl_modes.py` | pass 7 s | pass 7 s | 1.0 | ok |
| `scripts/verify_phase10d_separable.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase10d_threshold_modes.py` | pass 6 s | pass 7 s | 0.9 | ok |
| `scripts/verify_phase9a.py` | pass 18 s | pass 16 s | 1.2 | ok |
| `scripts/verify_phase9b.py` | fail 4 s | fail 4 s | n/a | fails on both |
| `scripts/verify_phase9c_quality.py` | pass 16 s | pass 16 s | 1.0 | ok |
| `scripts/verify_phase9d.py` | pass 3 s | pass 3 s | 1.0 | ok |
| `scripts/verify_phase9d_native.py` | pass 6 s | pass 6 s | 1.1 | ok |
| `scripts/verify_phase9e.py` | pass 5 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase9e_lifecycle.py` | pass 3 s | pass 3 s | 1.1 | ok |
| `scripts/verify_phase9f.py` | pass 26 s | pass 25 s | 1.1 | ok |
| `scripts/verify_phase9g.py` | pass 30 s | pass 29 s | 1.0 | ok |
| `scripts/verify_phase9g_auxiliary.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures need a compiled Swift generator |
| `scripts/verify_phase9g_compat.py` | NOT RUN | NOT RUN | n/a | NOT RUN: the 9F baseline is not preserved |
| `scripts/verify_phase9h.py` | pass 18 s | pass 18 s | 1.0 | ok |
| `scripts/verify_phase9h_compat.py` | NOT RUN | NOT RUN | n/a | NOT RUN: the earlier-app baselines are not preserved |
| `scripts/verify_phase9i_assisted.py` | NOT RUN | NOT RUN | n/a | NOT RUN: needs a fresh exported workspace |
| `scripts/verify_phase9i_native.py` | NOT RUN | NOT RUN | n/a | NOT RUN: source comes from a compiled probe |
| `scripts/verify_phase9i_typed.py` | pass 3 s | pass 2 s | 1.1 | ok |
| `scripts/verify_phase8a.py` | fail 1 s | fail 1 s | n/a | fails on both |
| `scripts/verify_phase8b.py` | fail 0 s | fail 0 s | n/a | fails on both |
| `scripts/verify_phase8d_color.py` | pass 28 s | pass 24 s | 1.1 | ok |
| `scripts/verify_phase8e.py` | pass 16 s | pass 16 s | 1.0 | ok |
| `scripts/verify_phase8f.py` | pass 23 s | pass 22 s | 1.0 | ok |
| `scripts/phase6_initial_masks.py` | pass 6 s | pass 2 s | 2.7 | ok |
| `scripts/verify_phase6.py` | pass 89 s | pass 79 s | 1.1 | ok |
| `scripts/verify_phase6c.py` | pass 4 s | pass 4 s | 1.0 | ok |
| `scripts/verify_phase4.py` | fail 24 s | fail 23 s | n/a | fails on both |
| `scripts/verify_phase3.py` | NOT RUN | NOT RUN | n/a | NOT RUN: fixtures need a compiled Swift probe |
| `scripts/verify_phase2.py` | pass 2 s | pass 2 s | 1.1 | ok |
| `scripts/verify_phase1.py` | pass 2 s | pass 2 s | 1.0 | ok |

**Outcomes.** 77 `ok`, 1 `Release-only`, 11 `fails on both`, 11 `NOT RUN`. No drive was `fixed`, `flaky` or an `open trap`, and the budget cut did not apply.

**Traps.** No Debug run trapped: no `Assertion failed`, `Fatal error` or `Precondition failed` in any `app*.log` a run left behind (drives that delete their scratch on success left none), and no `Rasterize*.ips` appeared in `~/Library/Logs/DiagnosticReports`. So no Swift file changed, the 58 assert sites stand, and neither app was rebuilt after the sweep.

**Debug against Release.** The 77 drives that pass on both took 894 s on Debug and 816 s on Release in total. The largest ratio is 2.7 (`phase6_initial_masks.py`, 6 s against 2 s) and the largest difference 16 s (`verify_phase12b_actions.py`, 47 s against 31 s). A drive is slow when Debug is more than 3 × Release and more than 60 s longer; none is.

**Release-only.** `verify_phase12g_clipboard.py` passed on Release in 1114 s and reached the limit on Debug twice, with 17 of its 19 gates done. `sample` of the owned app every 45 s through the second Debug run and one extra Release run (not in the table; with the sampler attached it reached the limit as well) puts 25% of the Debug app's main-thread samples in `AgentServer.id(for:)` and 15% in `AgentServer.openDocuments()` (`AgentServer.swift`), against 4% and 4% on Release: `target(_:)` calls `id(for:)` once per open document, each call rebuilds a set of every open document, and the drive never closes one. It is on the list in the root `CLAUDE.md`. The same samples put 53% of the Release app's main-thread time (23% on Debug) in building a `JobProgressController` panel for every background wait, which is why the drive is long on both. `verify_phase4.py` is on that list because it measures time; its row is `fails on both` because the Release run fails too.

**Fails on both.** Each of the 11 failed the same check on both configurations, in both runs: a frozen tool-catalog count in 8B and 8C; `WRAPPER_WORDS` ending at wrapper 55 in 8A, which 11E imports; a reworded Pass Through refusal in two 10D drives; no JPEG 2000 reader in this machine's ImageMagick for 11D's gate D6; a tool list from before `put_pixels` in 12A's gate r9; a float straighten that is no longer refused in 12F-1's gate b5; a 16-filter stack that 9B expects to be refused; and `duplicate_layer` refused at the 2 GiB working limit in Phase 4's 48 MP corpus. None was fixed here.

**Not run.** Eleven drives have no inputs on this machine: their fixtures come from a compiled generator or `cargo test`, or their baseline from an earlier app that is not preserved. `verify_phase12f5_paint.py` ran against the preserved baseline `~/devel/rasterize-fixtures/phase12-receipts/12h/12f5-baseline`.

**Counted as a pass.** `verify_phase12e3c_psd.py --skip-photoshop` ends `INCOMPLETE` with exit 2 on both configurations: its Photoshop gates are not run and no gate fails (a failed gate exits 1). The table counts that exit as this invocation's pass.

**Side effect.** `verify_phase12f3_warp.py` rewrote `public-docs/src/content/docs/reference/mcp-tools.md`, which was stale; the change was reverted.

## Phase 4

Measured 2026-10-06 on the Phase 1 machine (Apple M2 Max, 12 logical CPUs), in the worktree on `c46f334` plus this phase's changes, in this order, nothing else building. 408 test binaries, 655 Swift files. Wall time, one run each; every probe edit was reverted. `make gate` returns status 2 for every outcome but a pass, so the last line is the result.

| Measurement | Command | Result | Target |
|---|---|---|---|
| First run, no stored state; the `Cargo.toml` edit rebuilt every test binary (123 s), the app only relinked | `make gate` | `gate: PASS 457.6s`: fmt 6.2, core-build 4.0, clippy 16.3, tests 430.7, app 7.4, launch 0.9 | under 300 s; cap 600 s |
| No change | `make gate` | `gate: PASS 0.7s`: core-build 0.3 s, every other step `skip` | every leg skipped |
| Only this file edited | `make gate` | `gate: PASS 0.3s`: core-build 0.1 s, every other step `skip` | nothing runs |
| Comment added to `app/Sources/CropTool.swift`, launch replaced | `make gate GATE_ARGS="--app /usr/bin/false"` | status 2, `gate: FAIL (launch) 159.4s`: fmt, clippy, tests `skip`; app 0 in 158.8 s; launch 1 | Rust leg skipped; launch named |
| The same tree | `make gate` | `gate: PASS 3.5s`: app 2.0 s, launch 0 | pass |
| A `#[used]` static added to `core/src/durable_file.rs` (library `shasum` `98f2f329552b` → `f4ba00e95d1e`); Swift recompiled too, after the revert above | `make gate` | `gate: PASS 413.3s`: clippy 15.1, tests 383.0 (75 s of it the rebuild), app 208.3, launch 0.9 | clippy and tests run, app relinked; under 300 s |
| Failing test and clippy warning in `core/tests/guide_tests.rs`, type error in `CropTool.swift` | `make gate` | status 2, `gate: FAIL (clippy tests app) 395.8s`: clippy 101, tests 1 (`failed binaries: guide_tests`), app 2, launch `skip` | non-zero, three steps named |
| `build/gate.lock` held by another process | `make gate GATE_ARGS="--wait 2"` | status 2 after 2 s, `gate: BLOCKED lock build/gate.lock`; `gate-times.tsv` and `gate-state.json` unchanged | blocked, nothing written |
| Whole suite, three runs in a row | `python3 scripts/run_tests.py` | 215.0 s, 213.0 s, 214.2 s; each `ok: 408 binaries`, exit 0 | no failure; about 220 s |
| Whole suite, serial | `make test-serial` | 361 s, exit 0, 408 `test result: ok`, no `Doc-tests` section | about 500 s |
| Final run, finished tree: both legs ran, Swift recompiled, no test binary rebuilt | `make gate` | `gate: PASS 228.5s`: fmt 6.4, core-build 0.1, clippy 4.8, tests 216.7, app 169.0, launch 0.9 | under 300 s |

- **Target.** Every gate run was under the 600 s cap. The 300 s target was met only by the run that rebuilt no test binary (228.5 s); the two that relinked all 408 took 457.6 s and 413.3 s. In those two the pooled binaries' summed run time was 580 s and 586 s, against 230–242 s in the four runs on unchanged binaries; Phase 5 of the plan (merged test binaries) is the planned remedy. In the final run the tests took 216.7 s beside a 169 s app build, against 213–215 s alone.
- **"png" assertion (plan item 4).** No code change. All eight sites already assert on the message with the path removed (`e7639f0`, `ce80ad9`, `295a75b`); the message prints the full path of the file being opened, and both red runs predate those commits. A run with "png" in `TMPDIR` passed (7 binaries, 69 tests).
- **Solo list.** The plan's eight, none added: no binary failed in any gate run or in the three `run_tests.py` runs, beside the app build or alone. The eight take 150 s of each run.
- **Hash inputs beyond the plan's list.** Rust: `scripts/run_tests.py`, `scripts/gate.py`. Swift: `agent/`, `VERSION`, `scripts/gate.py`, `scripts/app_under_test.py`, `scripts/verification_isolation.py`. `.md` files are left out of both.
- **`core-build` runs on every run** that gets past `wait`, so the hashed library is the one the app links; the plan's "nothing runs" case is one no-op `cargo build` (0.1–0.3 s).
