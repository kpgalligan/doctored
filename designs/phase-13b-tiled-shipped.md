# Phase 13B — Tiled backing and history: shipped runs

The settled record of Phase 13B's shipped runs, moved verbatim from [phase-13b-tiled-status.md](phase-13b-tiled-status.md). The status doc keeps the state, the survey, the plan, the handed items and `## Open items`; "archived original", the receipts and the files named without their path are as its head defines them.

- **Sections:** each shipped run has three. Its contract: what shipped, its home (`precision_image.rs` when none is named), what a later run must know. Its decisions: only those in force that no code states, or that other files cite; `None kept.` means the files named state the rest, or a later run replaced it. Its verification record: only a miss, gap or unrun check that still stands, with its bullet in `## Open items`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

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

Every byte source producer answers a `LayerRaster`, so a placed layer is its asset's tile plane under a fresh identity. Home: `layer_raster.rs`. The report records a retained source before `layer.pixels`, so shared tiles count under `embedded_source_pixels`.

## 13B-2c-3a contract

The persistence, export, thumbnail, canvas-image, plate-copy and promotion reads off the view; the `.rz` and WARP readers `unseeded`; the reopen dedup (`native_sources_byte::materialize_byte`). Home: `layer_raster.rs`.

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

`filled_rgba16` / `filled_rgba32_extended` and `LayerRaster::filled` (one tile per shape, no admission) for `add_layer_at`, the sourced shapes and the narrowing's masked plane; unseeded resample outputs; `precision_codec::encode` over runs. Home: `core/src/precision_image_fill.rs`. A filled layer retains and admits less while the plans charge the whole plane (13B-8).

## 13B-3e-1 contract

The typed export chain, the 16-bit PSD writer and the float writers on runs, rows and one rect; every export render and the style bake unseeded. `channels16` holds four planes (`w·h·8`) at once, inside `save_psd`'s admitted `canvas_bytes`.

## 13B-3e-2 contract

The typed live filter and the retained-source render on tile reads with unseeded outputs (`color16` through one row buffer). The 16-bit preview is the exception (D5).

## 13B-3e-3a contract

The six remaining whole readers on tile reads (the warp source as one transient); the scalar filter's gray image unseeded. An order-independent walk goes in tile-band order, an order-dependent one row-major; a `job.every` hoisted per run keeps the flat count (`EVERY_INTERVAL`, `core/src/job_control.rs`) but can sit 4096 + w − 1 samples apart on a contiguous plane.

## 13B-3e-3b contract

The raw develop's reads on runs; `EVERY_INTERVAL`; the 9f probe repaired. The develop inspection preview MISSED on tile reads and kept its view (D4).

## 13B-3e-4a contract

The host intakes and the RAW develop output unseeded; the geometry source on one transient; the FFI lend a private pinned copy (`Plane::lent`), NULL on an allocation failure; `dr_precision_image_storage_identity`. A lend pointer is never an identity.

## 13B-3e-4b contract

`from_rgba16_decoded` / `from_rgba32_extended_decoded` for every typed decoder (a bitwise-uniform plane is filled, `uniform_tiles`); the narrowing's byte planes unseeded; `OutputView::Seeded` removed. Reopen and PSD-open admission still charge the dense plane (13B-8).

## 13B-3e-4c-1 contract

`contiguous_rgba16/32` removed; the two previews read the crate-private `preview_view_rgba16`; tests migrated to owned copies, per-sample reads and the storage identity (`core/tests/preview_view_exception.rs` pins the view cache). No drive covered develop inspection; `verify_phase8d.py` does now.

## 13B-3e-4c-2a contract

`OutputView` and its nine parameters deleted; no f32 constructor seeds; the f32 view type deleted, so a typed view is u16 only (`core/src/contiguous_views.rs`).

## 13B-3e-4c-2b contract

Closed as a recorded residual: no tiling code landed. An f32 plane above `CONTIGUOUS_CACHE_BYTES` stays contiguous: `tiles_of` / `uniform_tiles` refuse above the bound, `filled_plane` falls back, `transient_rgba32` borrows it, a changed `with_rect` copies the buffer once (three `#[ignore]`d tests). Since the f32 tiling's phase 1 (`5ad3dbc`) only an ADOPTED plane does: `uniform_tiles`, `filled_plane`, the float reopen / TIFF open and `with_rect` tile at every size; since its phase 2 (`09e0b98`) adopted planes tile too (Contiguous = tile-store refusal only). The module doc is the home of the representation only; the premise is D1.

## Decisions

- **The 13C seam.** `Tile<S>` is opaque behind `samples()`, so a spilled or compressed tile variant lands without changing `TilePlane`.
- **Structural sharing only.** Tiles are shared by `Arc` identity between states derived from each other (and, inside one plane, among bitwise-uniform tiles of one shape: `filled`, `uniform_tiles`, `from_rows_shared_with_job`); content-addressed / interned tiles across documents are out of scope.

### 13B-2a decisions

None kept.

### 13B-2b decisions

- **The view bound is one projection's working set (1 GiB, 256 entries).** `CONTIGUOUS_CACHE_BYTES` / `CONTIGUOUS_CACHE_ENTRIES` were sized to hold a 3-layer 48 MP or 10-layer 24 MP byte document while the compositor read every byte layer's view per projection; since 13B-2c-1 it reads rows from the tiles and a projection leaves no view. A working set above the bound thrashes LRU (each read re-materializes every view); 13B-6's registry absorbs the bound and makes it working-set aware.
- **The host trim excludes the views** (`cache_bytes − contiguous_view_bytes` against the `cacheLimit` trim at working limit / 8, `RasterResources.checkCaches`), decided because the document trims after every projection and would otherwise evict exactly what the next projection re-read. The views stay `cache: true`, so `EditingState`'s retained figure still subtracts them.

### 13B-2c-1 decisions

None kept.

### 13B-2c-2a decisions

- **Inpaint narrowing (the one answer that moves).** A fill whose union window misses the layer answers `Ok(None)` before any solve (`core/src/doc_inpaint.rs`). Before, `hole_distance` / `inpaint_plane` ran per component and could `Err` (`inpaint_em.rs`'s no-eligible-source refusal, a cancel inside the solve, an allocation failure) before the write-back found nothing to write; those errors are no longer raised for a call that cannot move a byte, which answers NULL with no message. The checks ahead of the loop (`reach_x/y`, the hole cap, the source check, `reserve`) kept their place and order; A post-plan reserve charged by the union rect has since been added.

### 13B-2c-2b decisions

None kept.

### 13B-2c-2c decisions

None kept.

### 13B-2c-3a decisions

None kept.

### 13B-2c-3b decisions

None kept.

### 13B-3a decisions

None kept.

### 13B-3b-1 decisions

- **(c) Planes above `CONTIGUOUS_CACHE_BYTES` stay Contiguous** (an f32 plane above 67,108,864 px; u16 never): decided because the cache would refuse their view, so every residual whole-view bind would be an un-admitted whole copy. They were to tile once 13B-3c/3d/3e moved the reads off whole views; the reads moved (no f32 view exists since 13B-3e-4c-2a) and they still stay contiguous, by 13B-3e-4c-2b D1; since the f32 tiling's phase 1 (`5ad3dbc`) only adopted planes do (filled, uniform decoded, float reopen / TIFF open and `with_rect` planes tile at every size), and since its phase 2 (`09e0b98`) none does: adopted planes tile, each plan charging its copy. The view bound itself is 13B-6's.

### 13B-3b-2 decisions

None kept.

### 13B-3c-1 decisions

None kept.

### 13B-3c-2a decisions

None kept.

### 13B-3c-2b decisions

None kept.

### 13B-3c-2c-1 decisions

None kept.

### 13B-3c-2c-2 decisions

None kept. The 19-site Seeded / Unseeded classification with each site's readers is in the archived original; at `HEAD` the two kept preview seeds (`live_filter_evaluate.rs`, `raw_source_geometry.rs`) are the only seeded typed constructor sites in `core/src`.

### 13B-3d-1 decisions

None kept.

### 13B-3d-2 decisions

None kept.

### 13B-3d-3a decisions

None kept.

### 13B-3d-3b decisions

None kept.

### 13B-3e-1 decisions

None kept.

### 13B-3e-2 decisions

- **D5 — the preview MISS revert.** Rule: a read that misses its warm bound on tile reads after one fix pass reverts to the whole-view read, together with the seeded producer feeding it. So the 16-bit arm of `live_filter_preview::preview` binds `preview_view_rgba16` once per call, and `source16`'s filtered output (`core/src/live_filter_evaluate.rs`, the typed masked-cache entry) stays the seeded `from_rgba16`. The seed is kept for the app's edit → preview loop: the first preview after each edit is a cache hit. Residual: every typed live-filter edit seeds its filtered plane (`w·h·8`); an unfiltered, converted preview materializes `color16`'s or the promoted master's unseeded output per call. Ratio, bound and cause: the `13B-3e-2 preview MISS` bullet.

### 13B-3e-3a decisions

None kept.

### 13B-3e-3b decisions

- **D1 — the geometry source bind was deferred to 13B-3e-4** (done by 13B-3e-4a). A transient at the geometry's source bind (`core/src/raw_source_geometry.rs`; the tap walk's 13B-2c-3b rule) would have sat beside the host-intake seed that feeds it (`dr_precision_doc_from_rgba16` and `host_pixels.rs` build the base the app develops, and the app's RAW recipe passes it straight to the geometry), so the call would have held tiles + that seed + the transient, `w·h·8` more peak than before. So the bind kept its view until the host intakes were unseeded, when ONE transient replaced their seed. Per-tap `rgba16_at` reads are declined: a tap walk takes one transient, never per-tap tile reads (13B-2c-3b; 13C / 13A-latency), and the app's geometry is the filtered lens arm.
- **D4 — the preview MISS and its revert.** `PrecisionImage::develop_preview` (`core/src/develop_inspect.rs`) stays a whole-view read, bound once per call through `preview_view_rgba16`, and the geometry output (`core/src/raw_source_geometry.rs`) stays the seeded `from_rgba16`: the app feeds the geometry output to the preview through `layerSource(0)` (`RawDevelopPreviewSession.swift`), so unseeding it would make every such preview materialize the view. Each geometry answer seeds its output (`w·h·8`); a preview of an unseeded plane binds a `w·h·8` view it materializes on the first call. Ratio, bound and cause: the `13B-3e-3b preview MISS` bullet.

### 13B-3e-4a decisions

None kept.

### 13B-3e-4b decisions

None kept.

### 13B-3e-4c-1 decisions

- **D2 — the previews take the request's SECOND arm**: a narrow crate-private view-backed accessor, kept as an exception owned by 13A-latency. No new tile-read attempt: 13B-3e-2 (the live-filter preview, 1.23 × / 1.20 ×) and 13B-3e-3b (the develop preview, 1.34 × / 1.06 × / 1.05 ×) each MISSED after a fix pass, so the tile row read belongs to 13A-latency. `preview_view_rgba16` is the removed `contiguous_rgba16`'s body verbatim, `pub(crate)`, u16 only, so every answer, refusal sentence and position, checkpoint, report log, `.rz` byte and `views_held` stayed what it was.

### 13B-3e-4c-2a decisions

None kept.

### 13B-3e-4c-2b decisions

- **D1 — closed as a recorded residual; no tiling code lands.** A user decision after the implementer's STOP: the spec assumed a float document can hold an f32 raster above `CONTIGUOUS_CACHE_BYTES`, and it was found that it cannot. Evidence as decided: every document goes through `build` → `native_preflight`, whose float `finish_plan` (`core/src/precision_native_plan.rs`) charges `reopen_retained` ≥ D + 1 MiB plus `decode_scratch_bytes` ≥ D + 16·n + 1 MiB (D = Σ w·h·16 over the rasters), so D < 2^30 − 1 MiB and every document raster is inside `tiles_of`'s bound and already tiled; `add_layer_at` refuses before allocating (`preflight_raster_addition`), the RZCB decode refuses, and the v40 reader builds a document; measured, an 8200×8193 all-0.0 float TIFF open was refused, with the tiling code and without it. Both the inequality and that refusal held under the fixed 2 GiB working limit; the limit is RAM-scaled now (`process_working_limit()` in `core/src/precision_image.rs`, set by the host from `WorkingLimit.automatic`; `designs/working-limit-ram-scaled.md`), and whether a float document can hold such a raster above that limit is not measured: re-deciding this is the 13B-8 bullet's. What reached above the bound as decided: direct `from_rgba32` / `from_rgba32_extended` (tests, examples) and, code-derived and not measured, a transient f32 render over an enlarged canvas (`CanvasSize` keeps layer extents); both stay contiguous by rule, and the render output adopts its accumulator with no copy.
- **D5 — the typed geometry plan charged no input transient** (found while re-verifying the spec's D7): closed since; the `DONE` bullet of `## Open items`.

## Verification record (13B-1)

Standing: the 9H `current-layer-commands-ui` failure on a locked screen, here and in later runs (`OPEN`, owner unassigned).

## Verification record (13B-2a)

No standing miss.

## Verification record (13B-2b)

No standing miss.

## Verification record (13B-2c-1)

Standing: `regional_window` warm against the bound view (`OPEN`, 13A-latency).

## Verification record (13B-2c-2a)

No standing miss.

## Verification record (13B-2c-2b)

No standing miss.

## Verification record (13B-2c-2c)

No standing miss.

## Verification record (13B-2c-3a)

No standing miss.

## Verification record (13B-2c-3b)

Standing: the `develop_inspection` load flake (`OPEN`, owner unassigned).

## Verification record (13B-3a)

Standing: `regional_group_materialization_cancel` failed the bench 3 of 3 (`OPEN`, 13A-latency).

## Verification record (13B-3b-1)

Standing: typed op latency against the pre-tile library (`OPEN`, 13A-latency).

## Verification record (13B-3b-2)

No standing miss.

## Verification record (13B-3c-1)

No standing miss.

## Verification record (13B-3c-2a)

Standing: the scoped Layer histogram warm (`OPEN`, 13A-latency); the bench stall again (13B-3a's line).

## Verification record (13B-3c-2b)

No standing miss.

## Verification record (13B-3c-2c-1)

Standing: the warm repeated resample of one input plane (`OPEN`, 13A-latency).

## Verification record (13B-3c-2c-2)

Standing: the unseeded whole-layer Copy warm, a MISS (`OPEN`, 13A-latency).

## Verification record (13B-3d-1)

No standing miss.

## Verification record (13B-3d-2)

Standing: `apply_adjustment` warm, a MISS (`OPEN`, 13A-latency); the dab's canvas-proportional passes (`OPEN`, 13B-9).

## Verification record (13B-3d-3a)

No standing miss.

## Verification record (13B-3d-3b)

Standing: `save_after_flatten` warm, a MISS (`OPEN`, 13A-latency).

## Verification record (13B-3e-1)

Standing: `save_psd_flattened` and `save_native_float` warm, both MISSES (`OPEN`, 13A-latency).

## Verification record (13B-3e-2)

Standing: the `13B-3e-2 preview MISS` bullet (`OPEN`, 13A-latency).

## Verification record (13B-3e-3a)

No standing miss.

## Verification record (13B-3e-3b)

Standing: the `13B-3e-3b preview MISS` bullet (`OPEN`, 13A-latency); 9H's `current-layer-commands-ui` failed on an unlocked screen (`OPEN`, owner unassigned). `scripts/verify_phase8d.py` was stale (since re-qualified); `scripts/verify_phase8g.py` was not run (its Nikon D70s fixture was absent).

## Verification record (13B-3e-4a)

Standing: the first preview of a fresh unseeded plane (`OPEN`, 13A-latency).

## Verification record (13B-3e-4b)

No standing miss.

## Verification record (13B-3e-4c-1)

No standing miss.

## Verification record (13B-3e-4c-2a)

Standing: `phase13a_regional_latency` was not re-measured, here or in 13B-3e-4c-2b (`OPEN`, 13A-latency).

## Verification record (13B-3e-4c-2b)

No standing miss of its own; the above-bound probe suite is in the receipts.
