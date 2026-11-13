# Phase 13B — Tiled backing and history: shipped runs

The settled record of Phase 13B's shipped runs, moved from [phase-13b-tiled-status.md](phase-13b-tiled-status.md) (condensed 2026-10-30; the verbatim text is in the archived original). The status doc keeps the state, the survey, the plan, the handed items and `## Open items`; "archived original", the receipts and the files named without their path are as its head defines them.

- **Sections:** each shipped run has a contract section: what shipped, its home (`precision_image.rs` when none is named), what a later run must know. `## Decisions` keeps only the decisions in force that no code states, or that other files cite. A miss, gap or unrun check that still stands is an item of the status doc's `## Open items`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Runs

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

## 13B-1 contract

`TilePlane<S>`: 256×256 `Arc`-shared tiles for five sample types, the bitwise sharing latch, pointer-deduplicated accounting, per-tile cancellation. Home: `tile_store.rs`. Core-only.

## 13B-2a contract

`LayerRaster` and `LayerRasterKey`: `Layer.pixels`, `RetainedSource8.rendered` and the WARP binding's two rasters are that type, and both identity caches key on it. Home: `layer_raster.rs`.

## 13B-2b contract

The tiled representation, the bounded view cache (now `core/src/contiguous_views.rs`), whole-image writers through `rebuilt`, paint through `read_rect` + `with_rect`. Home: `layer_raster.rs`, the writer contract the typed side copies. `with_rect` seeds no view.

## 13B-2c-1 contract

`row_segments` / `sample_at`: the byte compositor, the regional tree and `scan_content_box` read tiles in place. Home: `tile_store.rs`, `layer_raster.rs`. The resource report is a point sample: `unique_retained_bytes` can move between two reports when another thread evicts views.

## 13B-2c-2a contract

The rect-bounded byte writers (heal, inpaint, paint planes, bucket fill, Clear, red-eye and the rest) and `LayerRaster::unseeded`. Home: `core/src/layer_rect.rs`. The rect bounds the read, the diff the copies.

## 13B-2c-2b contract

Tile-aware byte equality (`same_bits`, `equals_contiguous`, `same_bytes`, `equals_image`), `to_contiguous_with_job`, `JobContext::cancelled`. Home: `tile_store.rs`, `layer_raster.rs`. Equality takes no job.

## 13B-2c-2c contract

Every byte source producer answers a `LayerRaster`, so a RAW (placed) layer is its asset's tile plane under a fresh identity. Home: `layer_raster.rs`. The report records a retained source before `layer.pixels`, so shared tiles count under `embedded_source_pixels`.

## 13B-2c-3a contract

The persistence, export, thumbnail, canvas-image, plate-copy and promotion reads off the view; the native-format and WARP readers `unseeded`; the reopen dedup (`native_sources_byte::materialize_byte`). Home: `layer_raster.rs`.

## 13B-2c-3b contract

The last production `contiguous_image()` calls removed (13B-2 closed): a tap-walk input binds one transient `to_contiguous[_with_job]` copy; the geometry outputs and the retouch temp documents are `unseeded`. Home: `layer_raster.rs`.

## 13B-3a contract

The typed accessor (`to_rgba16/32`, `ptr_eq`, `same_samples`; the borrowed `as_rgba16/32` removed), every reader migrated. Its view accessor `contiguous_rgba16/32` was removed by 13B-3e-4c-1.

## 13B-3b-1 contract

The typed flip (`Plane<S>` tiled or contiguous, an identity token), the view cache moved to `core/src/contiguous_views.rs`, the handle-owned FFI lend, `plane_key()`, the tiling-transient term in `retained_source_render::work_plan`.

## 13B-3b-2 contract

The typed writers (`rebuilt_*`, `with_rect_rgba16/32`, `read_rect_rgba16/32`, `from_rgba16_unseeded`, `reidentified`); `precision_image.rs` split into `#[path]` children.

## 13B-3c-1 contract

The typed reads (`row_segments`, `sample_at`, `runs_rgba16/32`; one u16 read rule, `normalized16`) under the typed compositor, the regional typed tree and `image_equal`. No typed content-box scan exists.

## 13B-3c-2a contract

Every consumer of a typed projection handle on tile reads (`rgba16_at`, the quantizer, the tone map, stats, soft proof, compare, scoped analysis), its gates, checkpoints and reports unchanged.

## 13B-3c-2b contract

The projection output unseeded; Copy / Copy Merged, the narrow and structure previews and `baked_projection` on rows and runs. Its `OutputView` parameter was removed by 13B-3e-4c-2a.

## 13B-3c-2c-1 contract

`transient_rgba16/32` (one random-access read per operation, no view) under the resample inputs; the retouch plates and the selection source on rows. `canvas_samples` holds one canvas-sized `W·H·8` buffer the plan does not charge separately (decided: charging it would change admission answers).

## 13B-3c-2c-2 contract

The other 19 typed constructor sites classified; the Copy / Cut payload and `convert`'s cross-storage output unseeded; the 9g probe reader repaired. Copy still charges `2 · payload_bytes`.

## 13B-3d-1 contract

The typed paint rect writers (7D paint / erase / clone / heal, the five native tools, the float Cut's erase) over the plane forms of `core/src/layer_rect.rs`, their home. The one canvas-proportional pass a dab has left is the bbox scan of the caller's canvas-sized coverage.

## 13B-3d-2 contract

The in-place retouch (`core/src/precision_retouch_apply.rs`) and the lock restore (`unshared_rect`) as one-rect writers; the whole-image writers on runs + `rebuilt_*`; an f32 destructive adjustment refuses by name. Run walks are plain nested loops, never an iterator chain over the runs (measured up to 3.7 × slower).

## 13B-3d-3a contract

`rebuild_over` seeds nothing, and the merges, stamp, flatten, bake, artwork, warp output, Rasterize, paste conversion, retouch new layer and tone maps are unseeded. The byte `LayerRaster::new` / `rebuilt` still seed (13B-2b).

## 13B-3d-3b contract

`filled_rgba16` / `filled_rgba32_extended` and `LayerRaster::filled` (one tile per shape, no admission) for `add_layer_at`, the sourced shapes and the narrowing's masked plane; unseeded resample outputs; `precision_codec::encode` over runs. Home: `core/src/precision_image_fill.rs`. A filled layer retains and admits less while the plans charge the whole plane (a dropped 13B-8 item).

## 13B-3e-1 contract

The typed export chain, the 16-bit PSD writer and the float writers on runs, rows and one rect; every export render and the style bake unseeded. `channels16` holds four planes (`w·h·8`) at once, inside `save_psd`'s admitted `canvas_bytes`.

## 13B-3e-2 contract

The retained-source (RAW) render on tile reads with unseeded outputs (`color16` through one row buffer).

## 13B-3e-3a contract

The six remaining whole readers on tile reads (the warp source as one transient); the scalar filter's gray image unseeded. An order-independent walk goes in tile-band order, an order-dependent one row-major; a `job.every` hoisted per run keeps the flat count (`EVERY_INTERVAL`, `core/src/job_control.rs`) but can sit 4096 + w − 1 samples apart on a contiguous plane.

## 13B-3e-3b contract

The raw develop's reads on runs; `EVERY_INTERVAL`; the 9f probe repaired. The develop inspection preview MISSED on tile reads and kept its view (D4).

## 13B-3e-4a contract

The host intakes and the RAW develop output unseeded; the geometry source on one transient; the FFI lend a private pinned copy (`Plane::lent`), NULL on an allocation failure; `dr_precision_image_storage_identity`. A lend pointer is never an identity.

## 13B-3e-4b contract

`from_rgba16_decoded` / `from_rgba32_extended_decoded` for every typed decoder (a bitwise-uniform plane is filled, `uniform_tiles`); the narrowing's byte planes unseeded; `OutputView::Seeded` removed. Reopen and PSD-open admission still charge the dense plane (a dropped 13B-8 item).

## 13B-3e-4c-1 contract

`contiguous_rgba16/32` removed; the develop preview reads the crate-private `preview_view_rgba16`; tests migrated to owned copies, per-sample reads and the storage identity (`core/tests/preview_view_exception.rs` pins the view cache). No drive covered develop inspection; `verify_phase8d.py` does now.

## 13B-3e-4c-2a contract

`OutputView` and its nine parameters deleted; no f32 constructor seeds; the f32 view type deleted, so a typed view is u16 only (`core/src/contiguous_views.rs`).

## 13B-3e-4c-2b contract

Closed as a recorded residual: no tiling code landed. An f32 plane above `CONTIGUOUS_CACHE_BYTES` stays contiguous: `tiles_of` / `uniform_tiles` refuse above the bound, `filled_plane` falls back, `transient_rgba32` borrows it, a changed `with_rect` copies the buffer once (three `#[ignore]`d tests). Since the f32 tiling's phase 1 (`5ad3dbc`) only an ADOPTED plane does: `uniform_tiles`, `filled_plane`, the float reopen / TIFF open and `with_rect` tile at every size; since its phase 2 (`09e0b98`) adopted planes tile too (Contiguous = tile-store refusal only). The module doc is the home of the representation only; the premise is D1.

## Decisions

- **The 13C seam.** `Tile<S>` is opaque behind `samples()`, so a spilled or compressed tile variant lands without changing `TilePlane`.
- **Structural sharing only.** Tiles are shared by `Arc` identity between states derived from each other (and, inside one plane, among bitwise-uniform tiles of one shape: `filled`, `uniform_tiles`, `from_rows_shared_with_job`); content-addressed / interned tiles across documents are out of scope.

### 13B-2b decisions

- **The view bound is one projection's working set (1 GiB, 256 entries).** `CONTIGUOUS_CACHE_BYTES` / `CONTIGUOUS_CACHE_ENTRIES` were sized to hold a 3-layer 48 MP or 10-layer 24 MP byte document while the compositor read every byte layer's view per projection; since 13B-2c-1 it reads rows from the tiles and a projection leaves no view. A working set above the bound thrashes LRU (each read re-materializes every view); 13B-6's registry absorbs the bound and makes it working-set aware ([#10](https://github.com/kpgalligan/doctored/issues/10)).
- **The host trim excludes the views** (`cache_bytes − contiguous_view_bytes` against the `cacheLimit` trim at working limit / 8, `RasterResources.checkCaches`), decided because the document trims after every projection and would otherwise evict exactly what the next projection re-read. The views stay `cache: true`, so `EditingState`'s retained figure still subtracts them.

### 13B-2c-2a decisions

- **Inpaint narrowing (the one answer that moves).** A fill whose union window misses the layer answers `Ok(None)` before any solve (`core/src/doc_inpaint.rs`). Before, `hole_distance` / `inpaint_plane` ran per component and could `Err` (`inpaint_em.rs`'s no-eligible-source refusal, a cancel inside the solve, an allocation failure) before the write-back found nothing to write; those errors are no longer raised for a call that cannot move a byte, which answers NULL with no message. The checks ahead of the loop (`reach_x/y`, the hole cap, the source check, `reserve`) kept their place and order; A post-plan reserve charged by the union rect has since been added.

### 13B-3b-1 decisions

- **(c) Planes above `CONTIGUOUS_CACHE_BYTES` stay Contiguous** (an f32 plane above 67,108,864 px; u16 never): decided because the cache would refuse their view, so every residual whole-view bind would be an un-admitted whole copy. They were to tile once 13B-3c/3d/3e moved the reads off whole views; the reads moved (no f32 view exists since 13B-3e-4c-2a) and they still stay contiguous, by 13B-3e-4c-2b D1; since the f32 tiling's phase 1 (`5ad3dbc`) only adopted planes do (filled, uniform decoded, float reopen / TIFF open and `with_rect` planes tile at every size), and since its phase 2 (`09e0b98`) none does: adopted planes tile, each plan charging its copy. The view bound itself is 13B-6's ([#10](https://github.com/kpgalligan/doctored/issues/10)).

### 13B-3c-2c-2 decisions

The 19-site Seeded / Unseeded classification with each site's readers is in the archived original; at `HEAD` the one kept preview seed (`raw_source_geometry.rs`) is the only seeded typed constructor site in `core/src`.

### 13B-3e-3b decisions

- **D1 — the geometry source bind was deferred to 13B-3e-4** (done by 13B-3e-4a). A transient at the geometry's source bind (`core/src/raw_source_geometry.rs`; the tap walk's 13B-2c-3b rule) would have sat beside the host-intake seed that feeds it (`dr_precision_doc_from_rgba16` and `host_pixels.rs` build the base the app develops, and the app's RAW recipe passes it straight to the geometry), so the call would have held tiles + that seed + the transient, `w·h·8` more peak than before. So the bind kept its view until the host intakes were unseeded, when ONE transient replaced their seed. Per-tap `rgba16_at` reads are declined: a tap walk takes one transient, never per-tap tile reads (13B-2c-3b), and the app's geometry is the filtered lens arm.
- **D4 — the preview MISS and its revert.** `PrecisionImage::develop_preview` (`core/src/develop_inspect.rs`) stays a whole-view read, bound once per call through `preview_view_rgba16`, and the geometry output (`core/src/raw_source_geometry.rs`) stays the seeded `from_rgba16`: the app feeds the geometry output to the preview through `layerSource(0)` (`RawDevelopPreviewSession.swift`), so unseeding it would make every such preview materialize the view. Each geometry answer seeds its output (`w·h·8`); a preview of an unseeded plane binds a `w·h·8` view it materializes on the first call. On tile reads the preview measured `preview_fit` 1.338 ×, `preview_fit_clipping` 1.057 × and `preview_detail` 1.051 × after a fix pass, against ≤ 1.05 ×.

### 13B-3e-4c-1 decisions

- **D2 — the develop preview takes the request's SECOND arm**: a narrow crate-private view-backed accessor, kept as an exception. No new tile-read attempt: 13B-3e-3b's develop preview MISSED after a fix pass (D4). `preview_view_rgba16` is the removed `contiguous_rgba16`'s body verbatim, `pub(crate)`, u16 only, so every answer, refusal sentence and position, checkpoint, report log, native-format byte and `views_held` stayed what it was.

### 13B-3e-4c-2b decisions

- **D1 — closed as a recorded residual; no tiling code lands.** A user decision after the implementer's STOP: the spec assumed a float document can hold an f32 raster above `CONTIGUOUS_CACHE_BYTES`, and it was found that it cannot. Evidence as decided: every document goes through `build` → `native_preflight`, whose float `finish_plan` (`core/src/precision_native_plan.rs`) charges `reopen_retained` ≥ D + 1 MiB plus `decode_scratch_bytes` ≥ D + 16·n + 1 MiB (D = Σ w·h·16 over the rasters), so D < 2^30 − 1 MiB and every document raster is inside `tiles_of`'s bound and already tiled; `add_layer_at` refuses before allocating (`preflight_raster_addition`), the RZCB decode refuses, and the v40 reader builds a document; measured, an 8200×8193 all-0.0 float TIFF open was refused, with the tiling code and without it. Both the inequality and that refusal held under the fixed 2 GiB working limit; the limit is RAM-scaled now (`process_working_limit()` in `core/src/precision_image.rs`, set by the host from `WorkingLimit.automatic`; `designs/working-limit-ram-scaled.md`), and the f32 tiling (`5ad3dbc`, `09e0b98`, Closed open items below) since tiles every f32 plane at every size. What reached above the bound as decided: direct `from_rgba32` / `from_rgba32_extended` (tests, examples) and, code-derived and not measured, a transient f32 render over an enlarged canvas (`CanvasSize` keeps layer extents); both stay contiguous by rule, and the render output adopts its accumulator with no copy.
- **D5 — the typed geometry plan charged no input transient** (found while re-verifying the spec's D7): closed since (Closed open items below).

## Closed open items

- `DONE` — split from 13B-8 — f32 planes above `CONTIGUOUS_CACHE_BYTES` (67,108,864 px), reachable at the 8 GiB limit of a 16 GB Mac (≤ 100 MP by `native_preflight`). **Phase 1** (`5ad3dbc`): every plane built without an adopted `Vec` tiles at every size (`filled_plane`, `uniform_tiles`, the float native reopen and float TIFF open via `from_rgba32_tiles_decoded`, every `with_rect` output). **Phase 2** (`09e0b98`): `tiles_of` has no bound, so every adopted f32 plane tiles too; Contiguous is a tile-store refusal only (test seam `contiguous_rgba32_extended`). Each adoption copy is charged or removed at every size: float render finalization `F32_FINALIZATION_BYTES_PER_PIXEL` = 41 (K fitted from 40), the float reopen figure + `FLOAT_REOPEN_MARGIN_PER_PIXEL` = 9 per float raster pixel (from 8), float paint's grown arm builds into tiles, the `convert` test seam charges 2·count·bpp; the float TIFF open covers 1.45× unchanged. Measured at 24 MP and 100 MP (records: `precision_render`, `precision_native_plan`, `float_io` module docs; `tests/float_above_cache_bound.rs`).
- `DONE` — The byte `adding_layer_at` zero plane (was 13B-5's): `adding_layer` / `adding_layer_at` build through `transparent_canvas_layer` over `LayerRaster::filled`.
- `DONE` — The typed geometry plan charged no input transient (was 13B-8's): `source_copy_bytes`, `adoption_copy_bytes`, `float_source_copy_bytes` and `float_adoption_copy_bytes` (`core/src/precision_sampling.rs`) are in the plan.
- `DONE` — `evict_caches`' `scope` string (`core/src/resources.rs`) says it drops every contiguous layer view and that `removed_cache_bytes` counts only the supplied documents' views.
- `DONE` — `develop_inspection::histogram_and_full_cell_marker_scans_cancel_and_release_reservations` parks the worker (`cancelled_parked_at`) instead of racing a poller, so its load failure is gone.
- `DONE` (2026-11-12) — 13B-7's plan item: `dr_doc_render_region_plan` / `dr_precision_doc_render_region_plan` (`core/src/ffi_regional.rs`) return the plan JSON, and MCP `render` with `region` or `preview_divisor` adds `{"regional_plan": {…}}` without the `layers` rows (`AgentServer+RegionalPlan.swift`), read before the render so a refusal surfaces once. Drive: `scripts/verify_regional_plan.py` P1–P7.
- `DONE` (2026-11-12) — the regional latency item (13B-2c-1's unmeasured warm 1.06–1.10×): re-measured by 13A-latency, the per-sample tile reads cost nothing at divisor 1 (tiled / contiguous 0.98–1.01) and 1.30–1.35× on a divisor-8 preview's strided reads, the residual stated in the [13A ADR](phase-13a-regional-status.md#measured-latency).

## Verification records

Run logs, tables and receipts: the archived original and the receipts folder. A miss, gap or unrun check that still stands is an item of the status doc's [`## Open items`](phase-13b-tiled-status.md#open-items); the dropped ones keep their figures in the archived original. 13B-3e-4c-2b's above-bound probe suite is in the receipts.
