# Phase 13B — Tiled backing and history: status

Section **13B** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("13. Scale to large production documents").

- **State:** 30 runs were implemented 2026-09-25 to 09-26; 13B-4, 13B-5, 13B-6 and 13B-9 are planned ([#10](https://github.com/kpgalligan/doctored/issues/10)); of 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)) MCP `render`'s plan shipped 2026-11-12, the canvas display waits for the owner's display-model decision and the packaged base's guided member is open; 13B-8 is dropped; 13B is not closed. Open items: [#9](https://github.com/kpgalligan/doctored/issues/9), [#10](https://github.com/kpgalligan/doctored/issues/10), [#11](https://github.com/kpgalligan/doctored/issues/11).
- **Original:** [archive/2026-10/phase-13b-tiled-status.md](../archive/2026-10/phase-13b-tiled-status.md) holds the full plan table and survey, every contract, all 198 decisions, the verification records with their tables, and the 48 closed open items.
- **Contract:** the module docs of `core/src/tile_store.rs`, `core/src/layer_raster.rs`, `core/src/contiguous_views.rs` and `core/src/precision_image.rs`, named below without their path. Measurements: `core/examples/phase13b_tile_sharing.rs`.
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13b/`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Survey — retained storage and undo today (before 13B-1)

What 13B-4, 13B-5 and 13B-6 start from, as `HEAD` has it (the before-13B-1 table and both blast-radius counts: archived original).

- 13B-4: a byte layer mask is `Layer.mask: Option<Arc<GrayImage>>` (`core/src/doc.rs`), a byte channel `Channel.data: Arc<GrayImage>` (`core/src/doc_channel.rs`), a typed mask or channel a `PrecisionPlane { pixels: Arc<Vec<u16>> }` (`core/src/precision_mask.rs`); the per-layer derived and aligned mask caches are unbounded (`core/src/doc_mask.rs`).
- 13B-5: `levelsOfUndo` is 0; the history budget defaults to `workingLimit / 4` incremental bytes (settable per document) and at most 4096 transitions (`DocumentHistory.swift`), measured by `EditingState.retainedBytes(in:)` over all states' handles in one report; named snapshots are at most 32 inside `workingLimit / 4` (`NamedSnapshots.swift`).
- 13B-6: the style cache holds `CACHE_CAPACITY` 4 planes per style, bytes unbounded (`core/src/style_cache.rs`); `MAX_CUBE_BYTES` is 64 MiB (`core/src/adjust_lut.rs`), `CONTENT_BOX_MEMO_CAPACITY` 512 entries (`core/src/doc_group_query.rs`), `process_cache_limit()` working limit / 16 (`core/src/cache_limit.rs`), the view cache `CONTIGUOUS_CACHE_BYTES` 1 GiB and 256 entries.
- The working limit is `process_working_limit()` (`core/src/precision_image.rs`): it starts at `REFERENCE_WORKING_BYTES` (2 GiB) and is RAM-scaled by the host from `WorkingLimit` (`app/Sources/WorkingLimit.swift`: automatic `min(ram/2, max(ram/3, ram − 8 GiB))`, a preference, or a test override). `MAX_PIXELS` is 100,000,000.

## Sub-slice plan

The six runs not shipped, verbatim:

| Run | Owns | Exit |
| --- | --- | --- |
| **13B-4** ([#10](https://github.com/kpgalligan/doctored/issues/10)) | Byte masks/channels (`Arc<GrayImage>`) and typed `PrecisionPlane` → `TilePlane<u8>` / `<u16>`; mask derived/aligned caches per tile. | Mask/channel suites green; byte-identical persistence; mask-paint dab retains ≤ 4 tiles. |
| **13B-5** ([#10](https://github.com/kpgalligan/doctored/issues/10)) | History: measured per-edit incremental bytes (history report `incremental_history_bytes`) before/after for brush, mask-paint, adjust and move on the 24 MP corpus; `NamedSnapshots` budget re-measured. Recovery / `.drsnapshot` on-disk sharing stays 13C ([#11](https://github.com/kpgalligan/doctored/issues/11)). | Measured before/after figures recorded with load; budget decision recorded. |
| **13B-6** ([#10](https://github.com/kpgalligan/doctored/issues/10)) | One process-wide byte-bounded resident-cache registry (style planes, mask caches, LUT, content-box) with eviction order and a report section, shared across documents/jobs. | Every cache bounded in bytes; report section present; eviction order tested. |
| **13B-7** ([#9](https://github.com/kpgalligan/doctored/issues/9)) | The regional path for the canvas display / UI previews (HELD for the owner: the canvas draws one canvas-sized image from the full projection `refreshProjection` renders on every edit, read by about 42 files, so a regional canvas needs a lazy projection, a display model only the owner can choose); the packaged byte base's guided member (open: its guide is built per 256-row strip of a window known only after the style's planes render, so it needs a per-strip stage and exact plan-time geometry). MCP `render`'s plan shipped 2026-11-12 (`## Shipped runs`). | Owned MCP drive; regional display equals full projection. |
| **13B-8** | `DROPPED` (won't-do under the 16 GB minimum; the f32 item shipped separately). Tighter regional plan/progress figures (split at its spec). | Each handed figure tightened or its residual stated with cause. |
| **13B-9** ([#10](https://github.com/kpgalligan/doctored/issues/10)) | Sampling plates. | Plates regional or their full-frame cost stated and bounded. |

### The 16 items handed from 13A

None moves outside 13B.

| # | Item (from the [13A ADR](phase-13a-regional-status.md#open-items)) | Placed in |
| --- | --- | --- |
| 1 | MCP `render` exposing the regional plan | 13B-7, shipped 2026-11-12 |
| 2 | Canvas display on the regional path | 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)) |
| 3 | Packaged byte base's guided member | 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9)) |
| 4 | `Global` style row's layer-rect input region | `DROPPED` with 13B-8 |
| 5 | Typed materialization sums / double charge + byte uncharged term | `DROPPED` with 13B-8 |
| 6 | Clip unit re-run figure | `DROPPED` with 13B-8 |
| 7 | Nested last stage at divisor 1 | `DROPPED` with 13B-8 |
| 8 | `rows_k` / `input_halo` | `DROPPED` with 13B-8 |
| 9 | HOLD 16 B/px never dropped / never preview-sampled | `DROPPED` with 13B-8 |
| 10 | Inside-an-effect sub-ranges | `DROPPED` with 13B-8 |
| 11 | Styled clip base member-aware split | `DROPPED` with 13B-8 |
| 12 | PSD bake ramp | `DROPPED` with 13B-8 |
| 13 | Guided sampling plates full-frame | 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)) |
| 14 | `plate_arm` predicate parse uncharged | 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)) |
| 15 | Retained parsed `Adjustment` LUT tables | 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)) |
| 16 | Sampling plate progress | 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)) |

The full text of each item is in the [13A archived original](../archive/2026-10/phase-13a-regional-status.md).

## Shipped runs

The run table, contracts, `## Decisions` (the decisions in force) and the closed open items are in [phase-13b-tiled-shipped.md](phase-13b-tiled-shipped.md); a run's sections go there in the batch that ships it.

## Open items

Verified 2026-10-07; fates set 2026-10-30. One bullet per standing row of the archived original, plus the residue its sections left; its 48 closed rows are history.

- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — owner 13B-7 — Handed 13A items 2 and 3 (the table above; item 1 shipped, 4–12 were dropped with 13B-8, 13–16 are 13B-9's): (2) the regional path is not used for the canvas display or UI previews, held for the owner's display-model decision (the 13B-7 row); (3) a guided member of a PACKAGED byte base (`composite_package_into`'s per-strip member loop) refuses by name, "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)" (13A-4b-ii-b-1/-2); tiled backing.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — owner 13B-4 — Byte masks/channels and typed planes onto tiles; per-tile mask caches (the plan row). The mask and channel raw-address test oracles (`plane().samples().as_ptr()`, seven suites) wait for it (13B-3b-1).
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — owner 13B-5 — Measured history/snapshot sharing and budget re-measure (the plan row).
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — owner 13B-6 — Byte-bounded shared resident-cache registry (the plan row).
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — owner 13B-6 — The 1 GiB contiguous-view bound (a projection working set above it re-materializes every layer), the cap itself (`CONTIGUOUS_CACHE_BYTES`); `LayerRaster::contiguous_image` is kept for the test oracles and this run.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — owner 13B-9 — The canvas-proportional passes an in-place retouch dab still pays after 13B-3d-2 (no plane read, no view): the source plate (the Current arm's canvas-sized row copy), the source transform (a canvas-sized sampled plane), and the scans of the canvas-sized coverage (selection domain, admitted-bounds check, `any`, the kernel's bbox); the scans, like a paint dab's coverage bbox scan (13B-3d-1), need a rect-shaped FFI input. Handed 13A items 13–16 are this run's too: (13) guided (staged) sampling plates stay full-frame: a guide-reading Shadows/Highlights keeps `flattened_cut_with_job` (per-tile stages would re-run each guide over halo-grown tiles); (14) `inpaint_source::plate_arm`'s predicate pass parses each contributing entry's `from_meta` under a MODELLED profile before any arm's reservation (all three arms), uncharged; (15) the tree's RETAINED parsed `Adjustment`s (a LUT table per row) are covered only by the parse's ×256 factor; (16) the sampling plate reports no progress: its tile loop, tree and full-frame fallback run under `checkpoints_only()`; a per-tile / per-row report.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — Warm cost of the unseeded whole-layer Copy: `typed/clipboard` `copy_layer` (24 MP u16) 1.16 × MISSED ≤ 1.10 ×; `copy_layer_float` 1.08 ×. Cause (traced in review, not re-timed): `tile_unseeded` (`precision_image_build.rs`) frees the adopted buffer inside the call, where the handle used to keep it as its view; `encode` is not the cause.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — Warm cost of the export writers' run walks after 13B-3e-1: `typed/exports` `save_psd_flattened` 1.08 × and `save_native_float` 1.28 × MISSED ≤ 1.05 ×: the warm call used to read the view its first call cached; `channels16` is about 1.1 × the slice passes, and chunking did not move `float_native::put_raster`'s staging fill from 4 KiB runs. The flattened PSD walks the projection twice (`channels16`, `composite_planes16`) and builds alpha twice; merging the walks needs a `composite_planes16` signature change.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — Typed op latency with the tiling copy and view materialization: `typed/dab` +29 %, `typed/projection` cold +30 % / warm +42 %, `typed/save` +27 % against the library before the typed flip, no bound; the constructors' tiling copy remains, never re-measured.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13C — Recovery / `.drsnapshot` on-disk tile sharing, spilling, compression.
- `DROPPED` — owner 13B-8, won't-do under the 16 GB minimum — Plans still charge the whole-plane scratch after 13B-3d-1: `paint_plan`'s `output_pixels·8` and the native tools' `n·24 + output·8` (the rect walks allocate only the rect); tightening them changes `admit` / `job.reserve` answers.
- `DROPPED` — owner 13B-8, won't-do under the 16 GB minimum — The filled plans' whole-plane charges: `StructureEdit::scratch_bytes`' AddLayer `w·h·bpp`, the promotion plan's `walk.shape` and the narrowing's `walk.output` still charge a dense plane where the filled plane allocates ≤ 4 tiles (13B-3d-3b D4), as do `reopen_retained_bytes` and the PSD open's `budget` for a uniform decoded layer (13B-3e-4b D2); the export plans (`export_plan`, `export_layer_plan`, `save_psd`'s `canvas_bytes`) were left unchanged when the exports moved onto tile reads (13B-3e-1).
- `DROPPED` (2026-10-30) — a plan-level declaration of the cancellation model (13A item, owner 13B): 13A's Decisions chose "No plan field", and it has no user-visible effect.
- `DROPPED` (2026-10-30) — tighter per-child progress figures inside a byte materialization's range (13A item, owner 13B): progress-bar smoothness only, dropped with 13B-8's sibling items.
- `DROPPED` (2026-10-30) — the warm per-edit copy of a transient input plane (`doc_warp::render_pixels`, `precision_warp.rs`, `raw_source_geometry`): no bound missed (0.98 ×, 1.006 ×).
- `DROPPED` (2026-10-30) — the kept preview seed: the RAW develop site (`raw_source_geometry.rs` with `develop_inspect.rs`, read through `preview_view_rgba16`) is a kept exception by decision (13B-3e-4c-1 D2).
- `DROPPED` (2026-10-30) — the typed transient resample input's warm copy (`geometry_rotate90_float` 1.21 ×): no bound applies, a small cost on a non-interactive op.
- `DROPPED` (2026-10-30) — `scoped_layer_histogram` 1.15 × warm after 13B-3c-2a: no bound applies, a small cost on a readout.
- `DROPPED` (2026-10-30) — the 13B-3e-3b develop inspection preview MISS on tile reads: the read kept its view and seed (13B-3e-3b D4), so no regression shipped.
- `DROPPED` (2026-10-30) — the whole-image writers' warm run walks after 13B-3d-2 (`apply_adjustment` 1.02 × against ≤ 1.00 ×): a 2 % miss on a no-regression bound; `retouch_dab` meets its bound.
- `DROPPED` (2026-10-30) — the typed native save's warm run walk (`save_after_flatten` 1.06 × at a falling load): a steady-load set read 0.86 ×, so the miss is load noise.
- `DROPPED` (2026-10-30) — `regional_group_materialization_cancel`'s "poller stalled past the stage" residual in the bench: an ignored wall-clock bench with a known load residual; the default test parks the worker.
- `DROPPED` (2026-10-30) — the seeded lend's second copy (`pinned_rgba16` beside a cached seed): inside its declared bound, and only test-built images and the kept RAW preview seed can double.
- `DROPPED` (2026-10-30) — the first preview of a fresh unseeded plane (`pipeline_preview_fit` 1.140 × on RAW intake): no bound applies, later previews hit the cache, and the decoder lines are inside ≤ 1.05 ×.
- `DROPPED` (2026-10-30) — partial re-sharing of an edited retained source's tiles on reopen (`core/tests/layer_raster_persistence.rs`, case (i)): placed images were removed, so it is a memory optimization for one RAW reopen case.
- `DROPPED` (2026-10-30) — `scripts/verify_phase9h_ui.py` `current-layer-commands-ui` fails ("The owned editor never became the real key window") on a locked screen, in four 13A verification records and in 13B's: the probe needs a real key window, and the case passes unlocked under `DR_BACKGROUND=1`; the one unlocked FAIL (13B-3e-3b) has not recurred.
- `DROPPED` (2026-10-30) — re-running `scripts/verify_phase8g.py` over the tiled develop reads (13B-3e-3b … 13B-3e-4c-1): a one-off re-verification of shipped code; the core suites stand for it.
