# Phase 13A — Regional evaluation: status

Section **13A** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("13. Scale to large production documents"), carryover [P7-31](phase-7-carryovers.md).

- **State:** every sub-slice but the deferred **13A-latency** ([#9](https://github.com/kpgalligan/doctored/issues/9)) was implemented 2026-09-23 to 09-25. 13A and P7-31 are open: both close when 13A-latency lands, or its residual is stated with cause. Open items: [#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11), [#22](https://github.com/kpgalligan/doctored/issues/22).
- **Original:** [archive/2026-10/phase-13a-regional-status.md](../archive/2026-10/phase-13a-regional-status.md) holds the full plan table, every contract delta, all 102 decisions, the verification records with their tables, and the struck open items.
- **Contract:** the module doc of `core/src/regional.rs` ("`regional.rs`" below); `core/MODULES.md`'s `regional.rs` entry lists the files (grep it, never read it whole).
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13a/`.

## Sub-slice plan (recorded verbatim from the 13A-1 spec)

The one run not shipped, verbatim:

| Run | Owns | Exit |
|---|---|---|
| **13A-latency** (DEFERRED — [#9](https://github.com/kpgalligan/doctored/issues/9)) | The regional render latency regression bisected in 13A-5b-iii-c-1 (first slower commit `b7dd168`, largest step `c214a1b` — Measured latency): profile the latency example's flat document, name the per-sample / per-render cost with evidence and remove it, keeping every regional exactness test green; check the typed path too. DEFERRED until the machine is idle (the fix is profiling + interleaved A/B timing against `68650dd`, which concurrent heavy jobs make meaningless). Must be done (or its residual stated with cause) before 13A closes | the latency example's 512×512 region / divisor-8 preview within 10% of the 13A-1 figures (≈13 ms; ≈18.6 / 37 ms), measured interleaved on an idle machine; output byte-identical to the reference |

State: **Deferred** — needs an idle machine ([#9](https://github.com/kpgalligan/doctored/issues/9)). The LAST 13A sub-slice: 13A and P7-31 close with it. The 13B status's regional-latency item is the same work.

## Shipped runs

The 31 shipped runs' commits, contracts, `## Decisions` (the decisions in force) and verification drives are in [phase-13a-regional-shipped.md](phase-13a-regional-shipped.md), moved there (condensed 2026-10-30; the verbatim text is in the archived original); a run's sections go there in the batch that ships it.

## 13A-6 cap survey (recorded in 13A-6a)

The caps that still stand (line numbers and per-megapixel thresholds: archived original):
- Selection: none. The typed Modify Selection kinds, Refine and the source selections run under a job control with no work refusal (`MAX_WORK_UNITS` 2·10⁹ bounded them before; it is now only the interactive preview budget, `core/src/precision_render.rs`). The byte selection transform stays at `MAX_PIXELS`; the byte feather `feather_mask` (`core/src/doc_select.rs`) has no cap.
- Artwork: `MAX_PATH_POINTS` 262,144, `MAX_PATH_EDGES` 524,288 (was 262,144), `MAX_SCAN_WORK` 256M (`core/src/artwork_coverage.rs`); the 64M aggregate, one extent plane per component (`artwork_render::MAX_COMPONENT_SAMPLES`); text 4,096 UTF-16 units (`app/Sources/ArtworkTextCapture.swift`, P7-34 / 13D, [#11](https://github.com/kpgalligan/doctored/issues/11)).
- Strokes: `MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 2·10⁹ (256M before; `core/src/brush_coverage.rs`), Smudge's own `MAX_SMUDGE_WORK` 256M; `MAX_RAW_LOCAL_DABS` 4,096 (`core/src/raw_local_coverage.rs`, 13D's, [#11](https://github.com/kpgalligan/doctored/issues/11)).

## Measured latency

The numbers 13A-latency ([#9](https://github.com/kpgalligan/doctored/issues/9)) starts from. `cargo run --release --example phase13a_regional_latency` renders the Phase 4 corpus stacked as three rasters (the corpus, then copies at (37, 21) Multiply 0.6 and (−15, 9) Screen 0.5) under one Levels adjustment; medians of 3; every regional output is checked equal to the full projection's crop before its time is reported.

13A-1 baseline (2026-09-23, `68650dd`): at 6000×4000 (24 MP) the full projection 1048.1 ms, the 512×512 region 13.1 ms, the divisor-8 preview of the whole canvas 18.5 ms; at 8000×6000 (48 MP) 2115.4, 12.8 and 37.0 ms.

Regression (13A-5b-iii-c-1, a quiet machine at load 4.6–7.3, two rounds interleaved with `68650dd`, which reproduced its baseline): region 28.8–29.2 ms at 24 MP and 27.8–28.6 ms at 48 MP, preview 41.2–41.9 and 81.1–81.4 ms, about 2.2× the baseline, with the full projection equal.

Bisect over `68650dd..057cbf5`: the first slower commit is `b7dd168` (13A-2a: +1.8 ms region, +3 ms 24 MP preview, ≈+14 %); `e597a6d` (13A-2b) adds ≈+1–2 ms; the largest step is `c214a1b` (13A-3a: +9–10 ms region, +13 ms 24 MP and +27 ms 48 MP preview, ≈+60 %); the later commits add ≈+2–3 ms together. The fixture uses none of the constructs those sub-slices admit, so the cost sits in shared regional machinery they changed; where was not investigated. The per-round and per-commit tables are in the archived original.

## Open items

Verified 2026-10-07; fates set 2026-10-30. One bullet per standing row of the archived original (owners 13A-latency, 13D and unassigned); its 65 struck rows are history. The pair-term bullet comes from the 13A-6b contract deltas.

- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — owner 13A-latency — The regional render of the latency example's flat document is ≈2.2× slower than 13A-1 at the 512×512 region (≈28–29 ms vs ≈13 ms) and the divisor-8 preview (≈41 / 81 ms vs ≈18.5 / 37 ms) with the full projection unchanged, a real cost, not load (`## Measured latency`, 13A-5b-iii-c-1): first slower commit `b7dd168` (13A-2a, ≈+1.8 ms region), largest step `c214a1b` (13A-3a, ≈+10 ms region); find the per-sample / per-render cost and remove it or state it, on an idle machine.
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — owner 13A-latency — Sample-major tree plate latency against the chain: T(1) / T(6) of the tree plate (429.5 / 965.2 ms in 13A-5c-2b's bench) beside 13A-5c-2a's fallback 114.6 / 283.3 ms, measured at unequal load; investigate if more than 3× at equal load.
- Handed to 13B: the 16 items of the 13B status's table "The 16 items handed from 13A" ([phase-13b-tiled-status.md](phase-13b-tiled-status.md#the-16-items-handed-from-13a)), which owns them: 1–3 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)), 4–12 dropped with 13B-8, 13–16 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)); and the two owner-13B items (a plan-level cancellation declaration, tighter per-child progress figures), dropped there.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — `MAX_WORK_UNITS` is not applied to the typed regional plan's `work_units` for a materialized subtree (the reference's `collect` refuses past it).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — Typed (16-bit / float) regional latency is not measured at 24/48 MP.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — `artwork_render`'s 64M aggregate (one EXTENT plane per component; `artwork_render::MAX_COMPONENT_SAMPLES`, `ffi_artwork.rs`, `NativeArtworkEvaluation.swift`): a windowed component contract (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The 50,000-dab cap (`MAX_BRUSH_DABS`) and `raw_local_coverage`'s 4,096 dabs: any cap number (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The byte feather (`feather_mask`, `core/src/doc_select.rs`) has no work cap at all (a depth-parity gap: the typed feather is now bounded by its support, the byte one by nothing) (targets).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The artwork scan's pair term (13A-6b left it unchanged): the row body's pair sweep charges at most `active²` per row, and exactly that when every active edge overlaps every other in y, so the 18,000-edge 1×1 outline still refuses at `MAX_SCAN_WORK` on the sweep and on `reference_rasterize` (`the_pair_term_still_refuses_on_both_paths`, `core/tests/bounded_artwork_coverage.rs`); the inner pair loop is still unpolled, one `every(i)` per outer edge (targets).
- `OPEN` [#22](https://github.com/kpgalligan/doctored/issues/22) — owner unassigned — The load-sensitive default tests the verification records call known flakes and never chased: `precision_layer_blending`, `refinement_cutout`, `agent_tests`, `phase8e_coverage`, `metadata_tests`, `bayesian_matting` (`content_aware_*` runs alone, `SOLO` in `scripts/run_tests.py`).
- `DROPPED` (2026-10-30) — owner 13D — Selection Transform (`taps²·n`), source selections (`40n`, charged by the selection `plan` in `core/src/precision_selection.rs`), the typed layer-mask feather check (`precision_mask.rs`) and Blend (`32n`) staying canvas-bounded: they read the whole canvas by nature, so there is nothing to build.
