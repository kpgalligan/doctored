# Phase 13A — Regional evaluation: status

Section **13A** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("13. Scale to large production documents"), carryover [P7-31](phase-7-carryovers.md).

- **State:** every sub-slice was implemented 2026-09-23 to 09-25, and the last, **13A-latency**, 2026-11-12: the 512×512 regional render is back under its 13A-1 time and the divisor-8 preview's residual is stated with its cause (`## Measured latency`), so every 13A run has shipped and carryover P7-31 is closed. Open items: [#11](https://github.com/kpgalligan/doctored/issues/11) (owner 13D), and the three items handed to 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)).
- **Original:** [archive/2026-10/phase-13a-regional-status.md](../archive/2026-10/phase-13a-regional-status.md) holds the full plan table, every contract delta, all 102 decisions, the verification records with their tables, and the struck open items.
- **Contract:** the module doc of `core/src/regional.rs` ("`regional.rs`" below); `core/MODULES.md`'s `regional.rs` entry lists the files (grep it, never read it whole).
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13a/`.

## Sub-slice plan

No run is left: 13A-latency, the last, shipped 2026-11-12 (its lines are in the shipped record). The typed regional path's latency is 13D's ([#11](https://github.com/kpgalligan/doctored/issues/11), `## Open items`).

## Shipped runs

The 32 shipped runs' commits, contracts, `## Decisions` (the decisions in force) and verification drives are in [phase-13a-regional-shipped.md](phase-13a-regional-shipped.md), moved there (condensed 2026-10-30; the verbatim text is in the archived original); a run's sections go there in the batch that ships it.

## 13A-6 cap survey (recorded in 13A-6a)

The caps that still stand (line numbers and per-megapixel thresholds: archived original):
- Selection: none. The typed Modify Selection kinds, Refine and the source selections run under a job control with no work refusal (`MAX_WORK_UNITS` 2·10⁹ bounded them before; it is now only the interactive preview budget, `core/src/precision_render.rs`). The byte selection transform stays at `MAX_PIXELS`; the byte feather `feather_mask` (`core/src/doc_select.rs`) has no cap.
- Artwork: `MAX_PATH_POINTS` 262,144, `MAX_PATH_EDGES` 524,288 (was 262,144), `MAX_SCAN_WORK` 256M (`core/src/artwork_coverage.rs`); the 64M aggregate, one extent plane per component (`artwork_render::MAX_COMPONENT_SAMPLES`); text 4,096 UTF-16 units (`app/Sources/ArtworkTextCapture.swift`, P7-34 / 13D, [#11](https://github.com/kpgalligan/doctored/issues/11)).
- Strokes: `MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 2·10⁹ (256M before; `core/src/brush_coverage.rs`), Smudge's own `MAX_SMUDGE_WORK` 256M; `MAX_RAW_LOCAL_DABS` 4,096 (`core/src/raw_local_coverage.rs`, 13D's, [#11](https://github.com/kpgalligan/doctored/issues/11)).

## Measured latency

The table 13A-latency was gated on. `cargo run --release --example phase13a_regional_latency -- --storage both` renders the Phase 4 corpus stacked as three rasters (the corpus, then copies at (37, 21) Multiply 0.6 and (−15, 9) Screen 0.5) under one Levels adjustment; medians of 3; every regional output is checked equal to the full projection's crop before its time is reported. `--storage contiguous` re-adopts every raster contiguous (`LayerRaster::shared`); `--loop N` repeats the regional calls for a profiler.

13A-1 baseline (2026-09-23, `68650dd`): at 6000×4000 (24 MP) the full projection 1048.1 ms, the 512×512 region 13.1 ms, the divisor-8 preview of the whole canvas 18.5 ms; at 8000×6000 (48 MP) 2115.4, 12.8 and 37.0 ms. 13A-5b-iii-c-1 measured the regression at ≈2.2× and bisected its largest step to `c214a1b` (13A-3a); the tables are in the archived original.

13A-latency (2026-11-12; Release; three rounds interleaving `68650dd`'s example built in the same session, the base `f4f93fb2` and the batch, no build running, 1-minute load 3.7–6.6; bound declared before measuring: each regional median within 1.10× the same round's `68650dd` figure). Ranges are the three rounds' medians, in ms:

| | `68650dd` | before (`f4f93fb2`) | after, tiled | after, contiguous |
|---|---|---|---|---|
| 24 MP region 512 | 12.5–12.7 | 36.4–36.5 | 10.9–11.0 | 10.8–10.9 |
| 24 MP preview ÷8 | 17.9–18.9 | 55.5–55.6 | 20.1–21.0 | 15.5–15.8 |
| 48 MP region 512 | 12.4–12.6 | 36.0–37.4 | 10.7–10.9 | 10.8–11.0 |
| 48 MP preview ÷8 | 35.8–36.1 | 111.6–115.7 | 40.9–41.5 | 30.7–31.0 |
| 24 / 48 MP full projection | 1019–1024 / 2036–2052 | 1758–1762 / 3520–3562 | 941–1008 / 1892–1921 | 945–962 / 1871–1891 |

The region is inside the bound (0.85–0.87×, round by round). The tiled preview MISSES it (1.10–1.17× at 24 MP, one round just inside; 1.13–1.16× at 48 MP); the residual is the tiled storage, not the compositor: the same binary's contiguous twin is 0.82–0.88× of `68650dd`, and only a strided read sees it (region tiled / contiguous 0.98–1.01; preview 1.30–1.35), because a divisor-8 row reads 32 samples from each 256-pixel tile, three layers at a time, where a contiguous row is one stream. The cause of the 3× (`/usr/bin/sample` on `--loop 400`): about half the regional time was the per-sample tree dispatch (`ResolvedNode::pixel` / `composite`, `ResolvedUnit::sample`, `ResolvedLevel::sample`), about 28 % the byte Levels' 8-bit form (`LevelsStage::apply8`'s f64 code arithmetic and `powf`, which also took the full projection from ≈1020 to ≈1760 ms), and 3 % the per-sample tile reads. The run-major walk alone gives 20.6–21.1 / 34.5–34.6 ms (24 MP region / preview) and the Levels memo alone 25.1 / 39.3–39.6; both are needed.

The tree plate against the chain, `make cancel-bench`'s calibrations (`a_concurrently_cancelled_tree_plate_returns_within_one_tree_row`, `a_concurrently_cancelled_fallback_returns_within_one_chain_row`), back to back in three rounds at load 8–14: before, the tree T(1) / T(6) 397–404 / 932–944 ms and the chain 407–412 / 565–573 ms (T_tree(6) / T_chain(6) ≈ 1.65); after, the tree 264–266 / 840–848 ms, the chain unchanged. Under the issue's 3× at equal load, so nothing further was built.

## Open items

Verified 2026-10-07; fates set 2026-10-30. One bullet per standing row of the archived original (owners 13A-latency, 13D and unassigned); its 65 struck rows are history. The pair-term bullet comes from the 13A-6b contract deltas.

- `DONE` (2026-11-12) — owner 13A-latency — The ≈2.2× regional slowdown: the region is back under 13A-1 and the preview's residual is the tiled storage's strided reads; the tree plate is 1.65× the chain at equal load, under 3× (`## Measured latency`).
- Handed to 13B: the 16 items of the 13B status's table "The 16 items handed from 13A" ([phase-13b-tiled-status.md](phase-13b-tiled-status.md#the-16-items-handed-from-13a)), which owns them: 1–3 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)), 4–12 dropped with 13B-8, 13–16 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)); and the two owner-13B items (a plan-level cancellation declaration, tighter per-child progress figures), dropped there.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — `MAX_WORK_UNITS` is not applied to the typed regional plan's `work_units` for a materialized subtree (the reference's `collect` refuses past it).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — Typed (16-bit / float) regional latency is not measured at 24/48 MP.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — `artwork_render`'s 64M aggregate (one EXTENT plane per component; `artwork_render::MAX_COMPONENT_SAMPLES`, `ffi_artwork.rs`, `NativeArtworkEvaluation.swift`): a windowed component contract (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The 50,000-dab cap (`MAX_BRUSH_DABS`) and `raw_local_coverage`'s 4,096 dabs: any cap number (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The byte feather (`feather_mask`, `core/src/doc_select.rs`) has no work cap at all (a depth-parity gap: the typed feather is now bounded by its support, the byte one by nothing) (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The artwork scan's pair term (13A-6b left it unchanged): the row body's pair sweep charges at most `active²` per row, and exactly that when every active edge overlaps every other in y, so the 18,000-edge 1×1 outline still refuses at `MAX_SCAN_WORK` on the sweep and on `reference_rasterize` (`the_pair_term_still_refuses_on_both_paths`, `core/tests/bounded_artwork_coverage.rs`); the inner pair loop is still unpolled, one `every(i)` per outer edge (targets).
- `DONE` (2026-11-12) — The load-sensitive default tests the September records called known flakes: `precision_layer_blending`, `refinement_cutout`, `agent_tests`, `phase8e_coverage`, `metadata_tests`, `bayesian_matting`. Their cancel tests park the worker (`common::probe::cancelled_parked_at`) and their timed bodies are `make cancel-bench` twins (build-velocity Phase 10); `agent_tests` binds a Unix socket in its own folder. The five suite binaries that mount them, run unfiltered and all five at once for ten rounds beside two other lanes' builds (1-minute load average 161 to 218 on 12 cores), passed 50 of 50 runs. `content_aware_*` runs alone (`SOLO` in `scripts/run_tests.py`).
- `DROPPED` (2026-10-30) — owner 13D — Selection Transform (`taps²·n`), source selections (`40n`, charged by the selection `plan` in `core/src/precision_selection.rs`), the typed layer-mask feather check (`precision_mask.rs`) and Blend (`32n`) staying canvas-bounded: they read the whole canvas by nature, so there is nothing to build.
