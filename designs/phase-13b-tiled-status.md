# Phase 13B — Tiled backing and history: status

Section **13B** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("## 13. Scale to large production documents").

- **State:** 30 runs were implemented 2026-09-25 to 09-26; 13B-4 … 13B-9 are planned; 13B is not closed.
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
| **13B-4** | Byte masks/channels (`Arc<GrayImage>`) and typed `PrecisionPlane` → `TilePlane<u8>` / `<u16>`; mask derived/aligned caches per tile. | Mask/channel suites green; byte-identical persistence; mask-paint dab retains ≤ 4 tiles. |
| **13B-5** | History: measured per-edit incremental bytes (history report `incremental_history_bytes`) before/after for brush, mask-paint, adjust and move on the 24 MP corpus; `NamedSnapshots` budget re-measured. Recovery / `.drsnapshot` on-disk sharing stays 13C. | Measured before/after figures recorded with load; budget decision recorded. |
| **13B-6** | One process-wide byte-bounded resident-cache registry (style planes, mask caches, LUT, content-box, live-filter) with eviction order and a report section, shared across documents/jobs. | Every cache bounded in bytes; report section present; eviction order tested. |
| **13B-7** | The regional path for the canvas display / UI previews; MCP `render` exposing the plan (`global_analysis`, `required_input_region`); the packaged byte base's guided member. | Owned MCP drive; regional display equals full projection. |
| **13B-8** | `DROPPED` (won't-do under the 16 GB minimum; the f32 item shipped separately). Tighter regional plan/progress figures (split at its spec). | Each handed figure tightened or its residual stated with cause. |
| **13B-9** | Sampling plates. | Plates regional or their full-frame cost stated and bounded. |

The 30 shipped runs (`Owns` / `Exit` cells and split lineage: archived original):

| Run | Commit | Implemented (2026) |
|---|---|---|
| 13B-1 | `c3e7a9d` | 09-25 |
| 13B-2a | `6989f63` | 09-25 |
| 13B-2b | `1fe7bfe` | 09-25 |
| 13B-2c-1 | `5bfc798` | 09-25 |
| 13B-2c-2a | `2454f6b` | 09-25 |
| 13B-2c-2b | `95e09b4` | 09-25 |
| 13B-2c-2c | `7add307` | 09-25 |
| 13B-2c-3a | `9dc0d3d` | 09-25 |
| 13B-2c-3b | `e7639f0` | 09-25 |
| 13B-3a | `09c7875` | 09-25 |
| 13B-3b-1 | `634746c` | 09-25 |
| 13B-3b-2 | `740294b` | 09-25 |
| 13B-3c-1 | `656a0aa` | 09-25 |
| 13B-3c-2a | `694194c` | 09-25 |
| 13B-3c-2b | `549b10e` | 09-25 |
| 13B-3c-2c-1 | `71e4873` | 09-25 |
| 13B-3c-2c-2 | `97e5825` | 09-25 |
| 13B-3d-1 | `a33e203` | 09-25 |
| 13B-3d-2 | `63421b3` | 09-26 |
| 13B-3d-3a | `e250ba6` | 09-26 |
| 13B-3d-3b | `2c04c47` | 09-26 |
| 13B-3e-1 | `3f7e2e5` | 09-26 |
| 13B-3e-2 | `a2541cf` | 09-26 |
| 13B-3e-3a | `5ec96ed` | 09-26 |
| 13B-3e-3b | `6b97c48` | 09-26 |
| 13B-3e-4a | `277c88a` | 09-26 |
| 13B-3e-4b | `9a30000` | 09-26 |
| 13B-3e-4c-1 | `31981c3` | 09-26 |
| 13B-3e-4c-2a | `d1f7601` | 09-26 |
| 13B-3e-4c-2b | `3a7cf21` | 09-26 (closed as a recorded residual) |

### The 16 items handed from 13A

None moves outside 13B.

| # | Item (from the [13A status](phase-13a-regional-status.md#open-items)) | Placed in |
| --- | --- | --- |
| 1 | MCP `render` exposing the regional plan | 13B-7 |
| 2 | Canvas display on the regional path | 13B-7 |
| 3 | Packaged byte base's guided member | 13B-7 |
| 4 | `Global` style row's layer-rect input region | 13B-8 |
| 5 | Typed materialization sums / double charge + byte uncharged term | 13B-8 |
| 6 | Clip unit re-run figure | 13B-8 |
| 7 | Nested last stage at divisor 1 | 13B-8 |
| 8 | `rows_k` / `input_halo` | 13B-8 |
| 9 | HOLD 16 B/px never dropped / never preview-sampled | 13B-8 |
| 10 | Inside-an-effect sub-ranges | 13B-8 |
| 11 | Styled clip base member-aware split | 13B-8 |
| 12 | PSD bake ramp | 13B-8 |
| 13 | Guided sampling plates full-frame | 13B-9 |
| 14 | `plate_arm` predicate parse uncharged | 13B-9 |
| 15 | Retained parsed `Adjustment` LUT tables | 13B-9 |
| 16 | Sampling plate progress | 13B-9 |

## Shipped runs

The contracts, `## Decisions` (the decisions in force) and verification records of the shipped runs are in [phase-13b-tiled-shipped.md](phase-13b-tiled-shipped.md), moved there verbatim; a run's sections go there in the batch that ships it.

## Open items

Verified 2026-10-07. One bullet per standing row of the archived original, plus the residue its sections left; its 48 closed rows are history.

- `OPEN` — owner 13A-latency — Warm-path cost of the transient input copy: a repeated edit on one input plane copies it per edit (`to_contiguous_with_job` in `doc_warp::render_pixels` and `live_filter_evaluate::source8`; `transient_rgba16/32` in `precision_warp.rs` and `raw_source_geometry`) where it hit the view cache. No bound missed; without one: `transforms/warm` `warp_edit#2` 0.98 ×, `live_filter_edit#2` 1.02 × (computed, one sample each), `typed/raw_intake` `pipeline_exif6` 1.006 ×. `source8` could walk the asset's runs at the cost of the copy's per-tile checkpoints (13B-3e-2 D4); a copy memo and per-tap tile reads are this owner's too.
- `OPEN` — owner 13A-latency — The Seeded sites classified in 13B-3c-2c-2 with another whole reader: all flipped but the two kept preview seeds (`live_filter_evaluate.rs`; `raw_source_geometry.rs` with `develop_inspect.rs`; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception).
- `OPEN` — owner 13A-latency — Warm cost of the unseeded whole-layer Copy: `typed/clipboard` `copy_layer` (24 MP u16) 1.16 × MISSED ≤ 1.10 ×; `copy_layer_float` 1.08 ×. Cause (traced in review, not re-timed): `tile_unseeded` (`precision_image_build.rs`) frees the adopted buffer inside the call, where the handle used to keep it as its view; `encode` is not the cause.
- `OPEN` — owner 13A-latency — Warm cost of the typed transient resample input: a repeated resample of one input plane copies it per call (`transform_image` / `resize_image`, `precision_sampling.rs`) where it hit the view cache: `typed/plates` `geometry_rotate90_float` 1.21 ×, no bound; the u16 lines ≤ 1.03 ×.
- `OPEN` — owner 13A-latency — Warm cost of `scoped_layer_histogram` after 13B-3c-2a: 1.15 × warm, 1.14 × cold (24 MP u16), no bound: a per-index `rgba16_at` through `mapped_xy` (`precision_scoped_source.rs`) inside `scoped_statistics::evaluate`'s `|i|` callback; removing it needs a row-shaped callback. The u16 histogram: 1.07 × warm, no bound.
- `OPEN` — owner 13A-latency — 13B-3e-2 preview MISS: the 16-bit live-filter preview on tile reads (one `row_segments` per output row, a forward cursor) measured `preview_after` warm 1.23 ×, 1.20 × after the fix pass, against ≤ 1.05 ×, so it kept its view and seed (13B-3e-2 D5). Cause (probes): a sampled row spans 8 tile runs of 2 KiB where the view is one contiguous 16 KiB row; neither a restructured cursor nor a burst row copy recovered it (the tile row read; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception).
- `OPEN` — owner 13A-latency — 13B-3e-3b preview MISS: the develop inspection preview on tile reads (a tile band walk, D4) measured `typed/raw_develop` `preview_fit` 1.375 ×, `preview_fit_clipping` 1.359 ×, `preview_detail` 1.249 ×, then 1.338 × / 1.057 × / 1.051 × after the fix pass, against ≤ 1.05 ×, so it kept its view and seed (D4). Cause: the per-column read through a tile row (the tile row read; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception).
- `OPEN` — owner 13A-latency — Warm cost of the whole-image writers' run walks after 13B-3d-2: `typed/whole_writers` `apply_adjustment` 1.02 × MISSED ≤ 1.00 × after the fix pass's tile-band changed scan (`precision_adjustment.rs`): the scan used to read a cached view (the base's seed). `typed/retouch_dab` 0.77 × missed its 0.75 × EXPECTATION (its ≤ 1.00 × bound MET).
- `OPEN` — owner 13A-latency — Warm cost of the export writers' run walks after 13B-3e-1: `typed/exports` `save_psd_flattened` 1.08 × and `save_native_float` 1.28 × MISSED ≤ 1.05 ×: the warm call used to read the view its first call cached; `channels16` is about 1.1 × the slice passes, and chunking did not move `float_native::put_raster`'s staging fill from 4 KiB runs. The flattened PSD walks the projection twice (`channels16`, `composite_planes16`) and builds alpha twice; merging the walks needs a `composite_planes16` signature change.
- `OPEN` — owner 13A-latency — Warm cost of the typed native save's run walk after 13B-3d-3b: `typed/producer_consumers` `save_after_flatten` 1.06 × MISSED ≤ 1.05 × at a falling load (0.86 × in a steady-load set, recorded only): the warm save used to read the view its first save cached; `precision_codec::encode` walks runs on every save.
- `OPEN` — owner 13A-latency — Typed op latency with the tiling copy and view materialization: `typed/dab` +29 %, `typed/projection` cold +30 % / warm +42 %, `typed/save` +27 % against the library before the typed flip, no bound (owner note: removed as 13B-3c / 3d / 3e take the copies away); the constructors' tiling copy remains, never re-measured.
- `OPEN` — owner 13A-latency — `regional_group_materialization_cancel`'s "poller stalled past the stage" residual failed 3 of 3 serial runs in 13B-3a, including one on the HEAD `e7639f0` library (previously ~1 in 3 under load) (the known 13A-5b-iii-b-1 load residual; not chased here)
- `OPEN` — owner 13A-latency — Regional render latency regression (13B-2c-1: `regional_window` warm 1.06–1.10 × with per-sample tile reads, cold 0.77 ×) (not re-measured here)
- `OPEN` — owner 13A-latency — The seeded lend's second copy: on an image built by a still-seeded constructor, while its seed is cached, the private lend (`pinned_rgba16`) is a second `w·h·8` beside the evictable seed (`peak_host_intake` `lend/seeded_u16`, inside its declared bound); only test-built images and the two kept preview seeds can double (the two kept preview seeds, `live_filter_evaluate.rs` / `raw_source_geometry.rs`, since 13B-3e-4c-1).
- `OPEN` — owner 13A-latency — The first preview of a fresh unseeded plane (an intake, a develop output, a decoded u16 master, an opened PNG16) materializes the view the constructor's seed used to supply: `typed/raw_intake` `pipeline_preview_fit` 1.140 ×, no bound; the two `typed/decoders` preview lines ≤ 1.040 ×, inside ≤ 1.05 ×; later previews hit the cache (the read is `preview_view_rgba16`, the kept exception, since 13B-3e-4c-1).
- `OPEN` — owner 13B-4 — Byte masks/channels and typed planes onto tiles; per-tile mask caches (the plan row). The mask and channel raw-address test oracles (`plane().samples().as_ptr()`, seven suites) wait for it (13B-3b-1).
- `OPEN` — owner 13B-5 — Measured history/snapshot sharing and budget re-measure (the plan row).
- `OPEN` — owner 13B-6 — Byte-bounded shared resident-cache registry (the plan row).
- `OPEN` — owner 13B-6 — The 1 GiB contiguous-view bound (a projection working set above it re-materializes every layer), the cap itself (`CONTIGUOUS_CACHE_BYTES`); `LayerRaster::contiguous_image` is kept for the test oracles and this run.
- `OPEN` — owner 13B-6 — `evict_caches` drops typed views too, but its `scope` string names only layer views and `removed_cache_bytes` counts only the supplied byte documents' views (`core/src/resources.rs`).
- `OPEN` — owner 13B-7 — The 16 handed 13A items (the table above): 1–3 here, 4–12 13B-8, 13–16 13B-9.
- `DROPPED` — owner 13B-8, won't-do under the 16 GB minimum — Plans still charge the whole-plane scratch after 13B-3d-1: `paint_plan`'s `output_pixels·8` and the native tools' `n·24 + output·8` (the rect walks allocate only the rect); tightening them changes `admit` / `job.reserve` answers.
- `DROPPED` — owner 13B-8, won't-do under the 16 GB minimum — The filled plans' whole-plane charges: `StructureEdit::scratch_bytes`' AddLayer `w·h·bpp`, the promotion plan's `walk.shape` and the narrowing's `walk.output` still charge a dense plane where the filled plane allocates ≤ 4 tiles (13B-3d-3b D4), as do `reopen_retained_bytes` and the PSD open's `budget` for a uniform decoded layer (13B-3e-4b D2); the export plans (`export_plan`, `export_layer_plan`, `export_scratch_bytes`, `save_psd`'s `canvas_bytes`) were left unchanged when the exports moved onto tile reads (13B-3e-1).
- `DONE` — split from 13B-8 — f32 planes above `CONTIGUOUS_CACHE_BYTES` (67,108,864 px), reachable at the 8 GiB limit of a 16 GB Mac (≤ 100 MP by `native_preflight`). **Phase 1** (`5ad3dbc`): every plane built without an adopted `Vec` tiles at every size (`filled_plane`, `uniform_tiles`, the float `.rz` reopen and float TIFF open via `from_rgba32_tiles_decoded`, every `with_rect` output). **Phase 2 shipped** (`09e0b98`): `tiles_of` has no bound, so every adopted f32 plane tiles too; Contiguous is a tile-store refusal only (test seam `contiguous_rgba32_extended`). Each adoption copy is charged or removed at every size: float render finalization `F32_FINALIZATION_BYTES_PER_PIXEL` = 41 (K fitted from 40), the float reopen figure + `FLOAT_REOPEN_MARGIN_PER_PIXEL` = 9 per float raster pixel (from 8), float paint's grown arm builds into tiles, the `convert` test seam charges 2·count·bpp; the float TIFF open covers 1.45× unchanged. Measured at 24 MP and 100 MP (records: `precision_render`, `precision_native_plan`, `float_io` module docs; `tests/float_above_cache_bound.rs`).
- `OPEN` — owner 13B-9 — The canvas-proportional passes an in-place retouch dab still pays after 13B-3d-2 (no plane read, no view): the source plate (the Current arm's canvas-sized row copy), the source transform (a canvas-sized sampled plane), and the scans of the canvas-sized coverage (selection domain, admitted-bounds check, `any`, the kernel's bbox); the scans, like a paint dab's coverage bbox scan (13B-3d-1), need a rect-shaped FFI input.
- `OPEN` — owner 13B, not placed in a run — A plan-level declaration of the cancellation model (the 13A record's `## Decisions`, "No plan field").
- `OPEN` — owner 13B, not placed in a run — Tighter per-child progress figures inside a byte materialization's range, which is flat at its start because the chain reports nothing (13A-5b-iii-b-2).
- `OPEN` — owner 13C — Partial re-sharing of an EDITED placed source on reopen (`rebuilt_from` over the asset): today a dab drops the layer's binding at save, so the reopened layer has no asset to share with (`core/tests/layer_raster_persistence.rs`, case (i)).
- `TRACKED → roadmap 13C` — Recovery / `.drsnapshot` on-disk tile sharing, spilling, compression.
- `OPEN` — owner unassigned — `scripts/verify_phase9h_ui.py` `current-layer-commands-ui` fails ("The owned editor never became the real key window") on a locked screen: the probe needs a real key window, and the case passes unlocked under `DR_BACKGROUND=1`. The one unlocked FAIL on record is 13B-3e-3b's (full run and alone); it has not recurred. Reopen only on an unlocked-screen FAIL.
- `OPEN` — owner unassigned — `develop_inspection::histogram_and_full_cell_marker_scans_cancel_and_release_reservations` failed twice under load ("inspection finished before live cancellation"; 13B-2c-3b, 13B-3c-2a): a race in the test, 0 of 30 at load on either library.
- `OPEN` — owner unassigned — `scripts/verify_phase8g.py` was not run over 13B-3e-3b … 13B-3e-4c-1, which moved the RAW develop's reads and previews (its Nikon D70s fixture was absent); the core suites stand for it.
- `DONE` — The byte `adding_layer_at` zero plane (was 13B-5's): `adding_layer` / `adding_layer_at` build through `transparent_canvas_layer` over `LayerRaster::filled`.
- `DONE` — The typed geometry plan charged no input transient (was 13B-8's): `source_copy_bytes`, `adoption_copy_bytes`, `float_source_copy_bytes` and `float_adoption_copy_bytes` (`core/src/precision_sampling.rs`) are in the plan.
