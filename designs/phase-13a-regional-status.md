# Phase 13A — Regional evaluation: status

Section **13A** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("## 13. Scale to large production documents"), carryover [P7-31](phase-7-carryovers.md).

- **State:** every sub-slice but the deferred **13A-latency** was implemented 2026-09-23 to 09-25. 13A and P7-31 are open: both close when 13A-latency lands, or its residual is stated with cause.
- **Original:** [archive/2026-10/phase-13a-regional-status.md](../archive/2026-10/phase-13a-regional-status.md) holds the full plan table, every contract delta, all 102 decisions, the verification records with their tables, and the struck open items.
- **Contract:** the module doc of `core/src/regional.rs` ("`regional.rs`" below); `core/MODULES.md`'s `regional.rs` entry lists the files (grep it, never read it whole).
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13a/`.

## Sub-slice plan (recorded verbatim from the 13A-1 spec)

The one run not shipped, verbatim:

| Run | Owns | Exit |
|---|---|---|
| **13A-latency** (DEFERRED — litecode run `phase-13a-regional-latency-regression`, request and triage written 2026-09-24) | The regional render latency regression bisected in 13A-5b-iii-c-1 (first slower commit `b7dd168`, largest step `c214a1b` — Measured latency): profile the latency example's flat document, name the per-sample / per-render cost with evidence and remove it, keeping every regional exactness test green; check the typed path too. DEFERRED until the machine is idle (the fix is profiling + interleaved A/B timing against `68650dd`, which concurrent heavy jobs make meaningless); the user runs it when nothing else is running. Must be done (or its residual stated with cause) before 13A closes | the latency example's 512×512 region / divisor-8 preview within 10% of the 13A-1 figures (≈13 ms; ≈18.6 / 37 ms), measured interleaved on an idle machine; output byte-identical to the reference |

State: **Deferred** — needs an idle machine; resume the litecode run `phase-13a-regional-latency-regression` at its design stage. The LAST 13A sub-slice: 13A and P7-31 close with it. 13A-latency also owns 15 rows of `## Open items` in [phase-13b-tiled-status.md](phase-13b-tiled-status.md#open-items).

The 31 shipped runs (`Owns` / `Exit` cells and split lineage: archived original):

| Run | Commit | Implemented (2026) |
|---|---|---|
| 13A-1 | `68650dd` | 09-23 |
| 13A-2a | `b7dd168` | 09-23 |
| 13A-2b | `e597a6d` | 09-23 |
| 13A-3a | `c214a1b` | 09-23 |
| 13A-3b-i | `6c9d050` | 09-23 |
| 13A-3b-ii | `8dbb083` | 09-23 |
| 13A-3c | `f859999` | 09-23 |
| 13A-4a | `32bfe66` | 09-23 |
| 13A-4b-i | `00397ec` | 09-23 |
| 13A-4b-ii-a | `843928e` | 09-23 |
| 13A-4b-ii-b-1 | `a5f2965` | 09-23 |
| 13A-4b-ii-b-2 | `0e39665` | 09-24 |
| 13A-5a | `1e3cf4e` | 09-24 |
| 13A-5b-i | `b9f7d75` | 09-24 |
| 13A-5b-ii-a | `fdad62c` | 09-24 |
| 13A-5b-ii-b | `15d5000` | 09-24 |
| 13A-5b-iii-a | `76a9aea` | 09-24 |
| 13A-5b-iii-b-1 | `5d19604` | 09-24 |
| 13A-5b-iii-b-2 | `057cbf5` | 09-24 |
| 13A-5b-iii-b-3 | `08aa01e` | 09-24 |
| 13A-5b-iii-c-1 | `d605faf` | 09-24 |
| 13A-5b-iii-c-2 | `dac3bf3` | 09-24 |
| 13A-5c-1 | `9683047` | 09-24 |
| 13A-5c-2a | `2704234` | 09-24 |
| 13A-5c-2b | `e47ace0` | 09-24 |
| 13A-5c-3 | `39f6a17` | 09-24 |
| 13A-6a | `58d4dbb` | 09-24 |
| 13A-6b | `b511129` | 09-24 |
| 13A-6c-1 | `49c8401` | 09-24 |
| 13A-6c-2 | `ab4664e` | 09-25 |
| 13A-7 | `576f69f` | 09-25 |

## Shipped runs

The contracts, `## Decisions` (the decisions in force) and verification records of the shipped runs are in [phase-13a-regional-shipped.md](phase-13a-regional-shipped.md), moved there verbatim; a run's sections go there in the batch that ships it.

## 13A-6 cap survey (recorded in 13A-6a)

The caps that still stand (line numbers and per-megapixel thresholds: archived original):
- Selection: none. The typed Modify Selection kinds, Refine and the source selections run under a job control with no work refusal (`MAX_WORK_UNITS` 2·10⁹ bounded them before; it is now only the interactive preview budget, `core/src/precision_render.rs`). The byte selection transform stays at `MAX_PIXELS`; the byte feather `feather_mask` (`core/src/doc_select.rs`) has no cap.
- Artwork: `MAX_PATH_POINTS` 262,144, `MAX_PATH_EDGES` 524,288 (was 262,144), `MAX_SCAN_WORK` 256M (`core/src/artwork_coverage.rs`); the 64M aggregate, one extent plane per component (`core/src/artwork_render.rs`); text 4,096 UTF-16 units (`app/Sources/ArtworkTextCapture.swift`, P7-34 / 13D).
- Strokes: `MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 2·10⁹ (256M before; `core/src/brush_coverage.rs`), Smudge's own `MAX_SMUDGE_WORK` 256M; `MAX_RAW_LOCAL_DABS` 4,096 (`core/src/raw_local_coverage.rs`, 13D's).

## Measured latency

`cargo run --release --example phase13a_regional_latency` renders the Phase 4 corpus stacked as three rasters (the corpus, then copies at (37, 21) Multiply 0.6 and (−15, 9) Screen 0.5) under one Levels adjustment; medians of 3; every regional output is checked equal to the full projection's crop before its time is reported.

13A-1 baseline (2026-09-23, `68650dd`): at 6000×4000 (24 MP) the full projection 1048.1 ms, the 512×512 region 13.1 ms, the divisor-8 preview of the whole canvas 18.5 ms; at 8000×6000 (48 MP) 2115.4, 12.8 and 37.0 ms.

Regression (13A-5b-iii-c-1, a quiet machine at load 4.6–7.3, two rounds interleaved with `68650dd`, which reproduced its baseline): region 28.8–29.2 ms at 24 MP and 27.8–28.6 ms at 48 MP, preview 41.2–41.9 and 81.1–81.4 ms, about 2.2× the baseline, with the full projection equal.

Bisect over `68650dd..057cbf5`: `68650dd` is the baseline; the first slower commit is `b7dd168` (13A-2a: +1.8 ms region, +3 ms 24 MP preview, ≈+14 %); `e597a6d` (13A-2b) adds ≈+1–2 ms; the largest step is `c214a1b` (13A-3a: +9–10 ms region, +13 ms 24 MP and +27 ms 48 MP preview, ≈+60 %); the later commits add ≈+2–3 ms together. The fixture uses none of the constructs those sub-slices admit, so the cost sits in shared regional machinery they changed; where was not investigated. The per-round and per-commit tables are in the archived original.

## Open items

Verified 2026-10-07. One bullet per standing row of the archived original (owners 13A-latency, 13B, 13D); its 65 struck rows are history. The pair-term bullet comes from the 13A-6b contract deltas; the last three `OPEN` bullets and the `DROPPED` one come from Decisions and the verification records.

- `OPEN` — owner 13A-latency — The regional render of the latency example's flat document is ≈2.2× slower than 13A-1 at the 512×512 region (≈28–29 ms vs ≈13 ms) and the divisor-8 preview (≈41 / 81 ms vs ≈18.5 / 37 ms) with the full projection unchanged, a real cost, not load (`## Measured latency`, 13A-5b-iii-c-1): first slower commit `b7dd168` (13A-2a, ≈+1.8 ms region), largest step `c214a1b` (13A-3a, ≈+10 ms region); find the per-sample / per-render cost and remove it or state it. Deferred to an idle machine: litecode run `phase-13a-regional-latency-regression`.
- `OPEN` — owner 13A-latency — Sample-major tree plate latency against the chain: T(1) / T(6) of the tree plate (429.5 / 965.2 ms in 13A-5c-2b's bench) beside 13A-5c-2a's fallback 114.6 / 283.3 ms, measured at unequal load; investigate if more than 3× at equal load.
- `TRACKED → 13B-7` (13B status, handed item 1) — MCP `render` does not expose the regional plan, so drives cannot read `global_analysis` / `required_input_region`.
- `TRACKED → 13B-7` (13B status, handed item 2) — Using the regional path for the canvas display or UI previews.
- `TRACKED → 13B-7` (13B status, handed item 3) — A guided member of a PACKAGED byte base (`composite_package_into`'s per-strip member loop) refuses by name, since 13A-4b-ii-b-2 "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)" (decided in 13A-4b-ii-b-1); tiled backing.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 4) — A layer-rect-bounded `required_input_region` for a `Global` style row (tighter than the canvas).
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 5) — The typed materialization terms are sums, not peaks, and a styled raster inside a typed materialized subtree is charged twice (conservative); the byte uncharged term charges `pkg` at `S` and a pass-through's crop at `A` and adds `guide(B)` beside the later `20·A` (conservative); tighter plan figures.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 6) — A clip unit holding a guided member is RE-RUN from its began sample in every later fill (charged: its stages count rows from the unit's first row, conservatively the level's descendants between too); a tighter per-stage figure.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 7) — A nested level's LAST stage is held at divisor 1 over window ∩ buffer even for a preview (stated cost).
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 8) — A nested level's `rows_k` count its descendants' rows too, and `input_halo` sums the halos of every stage above a row across sibling levels: conservative work / halo; tighter plan figures.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 9) — A HOLD is charged 16 bytes per `R_0` pixel for the whole staged render even when entered over a later, smaller region, and is never dropped after its last FINISH; a held plane is never sampled at the preview divisor; tighter plan figures.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 10) — Inside ONE effect, the effect's later blurs re-ramp under `fetch_max` and hold its sub-range's end (the stated residual of 13A-5b-iii-c-1's equal per-effect split, both depths); sub-ranges inside an effect.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 11) — A styled CLIP BASE's members render after its planes from `render_styled`'s own `completed`, so their reports sit below the planes' range end and hold under `fetch_max` until they pass it (the stated residual of 13A-5b-iii-c-2's `[completed, completed + work]` range, typed full frame and materialization alike); a member-aware split of the base's range.
- `DROPPED` (won't-do under the 16 GB minimum; 13B status, handed item 12) — The typed PSD bake (`bake_style16`) renders its planes under `job.range(0.2, 0.9).checkpoints_only()`, flat at 0.2, exactly as before 13A-5b-iii-c-2 (the narrowing moved to its call site); a ramp inside the bake's range.
- `TRACKED → 13B-9` (13B status, handed item 13) — Guided (staged) sampling plates stay full-frame: a guide-reading Shadows/Highlights keeps `flattened_cut_with_job` (per-tile stages would re-run each guide over halo-grown tiles).
- `TRACKED → 13B-9` (13B status, handed item 14) — `inpaint_source::plate_arm`'s predicate pass parses each contributing entry's `from_meta` under a MODELLED profile before any arm's reservation (all three arms), uncharged.
- `TRACKED → 13B-9` (13B status, handed item 15) — The tree's RETAINED parsed `Adjustment`s (a LUT table per row) are covered only by the parse's ×256 factor.
- `TRACKED → 13B-9` (13B status, handed item 16) — The sampling plate reports no progress (it never did): its tile loop, tree and full-frame fallback run under `checkpoints_only()`; a per-tile / per-row report.
- `OPEN` — owner 13D — `MAX_WORK_UNITS` is not applied to the typed regional plan's `work_units` for a materialized subtree (the reference's `collect` refuses past it).
- `OPEN` — owner 13D — Typed (16-bit / float) regional latency is not measured at 24/48 MP.
- `OPEN` — owner 13D — `artwork_render`'s 64M aggregate (one EXTENT plane per component; `artwork_render.rs:72`, `ffi_artwork.rs:120`, `NativeArtworkEvaluation.swift:69`): a windowed component contract (targets).
- `OPEN` — owner 13D — The 50,000-dab cap (`MAX_BRUSH_DABS`) and `raw_local_coverage`'s 4,096 dabs: any cap number (targets).
- `OPEN` — owner 13D — Selection Transform (`taps²·n`), source selections (`40n`, charged by the selection `plan` in `core/src/precision_selection.rs`), the typed layer-mask feather check (`precision_mask.rs:163`) and Blend (`32n`) stay canvas-bounded, by nature (a transform or source reads the whole canvas) or analysis-wide (targets).
- `OPEN` — owner 13D — The byte feather (`feather_mask`, `core/src/doc_select.rs`) has no work cap at all (a depth-parity gap: the typed feather is now bounded by its support, the byte one by nothing) (targets).
- `OPEN` — owner 13D — The artwork scan's pair term (13A-6b left it unchanged): the row body's pair sweep charges at most `active²` per row, and exactly that when every active edge overlaps every other in y, so the 18,000-edge 1×1 outline still refuses at `MAX_SCAN_WORK` on the sweep and on `reference_rasterize` (`the_pair_term_still_refuses_on_both_paths`, `core/tests/bounded_artwork_coverage.rs`); the inner pair loop is still unpolled, one `every(i)` per outer edge (targets).
- `OPEN` — owner 13B — A plan-level declaration of the cancellation model (`## Decisions`, "No plan field").
- `OPEN` — owner 13B — Tighter per-child progress figures inside a byte materialization's range, which is flat at its start because the chain reports nothing (13A-5b-iii-b-2).
- `OPEN` — owner unassigned — The load-sensitive default tests the verification records call known flakes and never chased: `precision_layer_blending`, `refinement_cutout`, `agent_tests`, `phase8e_coverage`, `metadata_tests`, `bayesian_matting` (`content_aware_*` runs alone, `SOLO` in `scripts/run_tests.py`).
- `DROPPED` — the `scripts/verify_phase9h_ui.py` `current-layer-commands-ui` activation failure in four verification records: the screen was locked each time; the drive passed unlocked (13A-5c-1).
