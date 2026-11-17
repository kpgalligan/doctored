# Phase 13B — Tiled backing and history: shipped runs

The settled record of Phase 13B's shipped runs, moved from [phase-13b-tiled-status.md](phase-13b-tiled-status.md) (condensed 2026-10-30; the verbatim text is in the archived original). The status doc keeps the state, the survey, the plan, the handed items and `## Open items`; "archived original", the receipts and the files named without their path are as its head defines them.

- **Sections:** each shipped run has a contract section: what shipped, its home (`precision_image.rs` when none is named), what a later run must know. `## Decisions` keeps only the decisions in force that no code states, or that other files cite. A miss, gap or unrun check that still stands is an item of the status doc's `## Open items`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Runs

The 34 shipped runs (`Owns` / `Exit` cells and split lineage: archived original; 13B-4's: its contract below):

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
| 13B-4 | `79a5f644` | 11-15 |
| 13B-5 | `31d4e145`…`1d3c2356` | 11-15 |
| 13B-6 | `e0a5a628` | 11-15 |
| 13B-9a | the commit that adds this row | 11-16 |

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

`from_rgba16_decoded` / `from_rgba32_extended_decoded` for every typed decoder (a bitwise-uniform plane is filled, `TilePlane::uniform_from`); the narrowing's byte planes unseeded; `OutputView::Seeded` removed. Reopen and PSD-open admission still charge the dense plane (a dropped 13B-8 item).

## 13B-3e-4c-1 contract

`contiguous_rgba16/32` removed; the develop preview reads the crate-private `preview_view_rgba16`; tests migrated to owned copies, per-sample reads and the storage identity (`core/tests/preview_view_exception.rs` pins the view cache). No drive covered develop inspection; `verify_phase8d.py` does now.

## 13B-3e-4c-2a contract

`OutputView` and its nine parameters deleted; no f32 constructor seeds; the f32 view type deleted, so a typed view is u16 only (`core/src/contiguous_views.rs`).

## 13B-3e-4c-2b contract

Closed as a recorded residual: no tiling code landed. An f32 plane above `CONTIGUOUS_CACHE_BYTES` stays contiguous: `tiles_of` / `TilePlane::uniform_from` refuse above the bound, `filled_plane` falls back, `transient_rgba32` borrows it, a changed `with_rect` copies the buffer once (three `#[ignore]`d tests). Since the f32 tiling's phase 1 (`5ad3dbc`) only an ADOPTED plane does: `TilePlane::uniform_from`, `filled_plane`, the float reopen / TIFF open and `with_rect` tile at every size; since its phase 2 (`09e0b98`) adopted planes tile too (Contiguous = tile-store refusal only). The module doc is the home of the representation only; the premise is D1.

## 13B-4 contract

Byte masks and channels (`Layer.mask`, `Channel.data`, the vector mask's raster part and baked plane, the WARP masks, the PATH table's byte planes) are `GrayRaster` (`core/src/gray_raster.rs`), typed masks and channels `PrecisionPlane` on `TilePlane<u16>` (`core/src/precision_mask.rs`): shared 256×256 tiles behind an identity token, no `Deref`, no `PartialEq`. Exit met (`core/tests/mask_plane_tiles.rs`): three 300×300 dabs on a 1536×1024 plane retain the base plane plus 4 tiles per dab (byte mask, byte channel, typed mask; 4 whole planes before); the saved native bytes are the row-major samples (a tile-order walk fails); the mask and channel suites are green.

Its transient copies, measured 2026-11-16 at 24 MP beside the plan that admits each operation (`cd core && cargo run --release --example phase13b_transient_copies`, peak heap above the baseline; the layered TIFF spot planes, copied before, are read in place inside the save's plan since the 13B-5 contract's layered-save lines):

| Copy (24 MP) | Plan scratch | Peak |
| --- | --- | --- |
| byte `move_selected` mask: the stored plane's `to_image()` | 436,194,304 | 48,043,050 |
| typed `move_selected` mask: `to_contiguous()` | 484,194,304 | 96,086,807 |
| typed 0.9× resize of a masked layer: `transform_plane_outside`'s copy, `from_u16` | 736,292,840 | 693,122,082 |
| typed `set_channel`: `from_u16` holding its input beside its tiles | 100,194,304 | 48,022,228 |
| byte Levels Auto over a masked adjustment: `scoped_auto`'s effective mask | 1,323,151,872 | 528,100,738 |

Each is inside its plan, so no plan gained a term. `fill_align_box` (both depths) and the byte group mask thumbnail's box arm had no plan and copied the mask (24,000,000 and 48,000,000 for the box); since 13B-9a they read it in place (`MaskCoverage::covered`; `doc_channel::placed_row` over the mask's own rows), and the box peaks at 0. The mask thumbnail's arm below the box threshold still copies the plane, bounded by that threshold.

## 13B-5 contract

What one edit retains, read from `list_history.retained_bytes` (the union of every state's handles; `scripts/verify_phase13b_history.py`, which asserts the fixes on 1500×1000 and prints the 24 MP rows with `--measure 8|16`). Four over-retentions found and fixed: a bitwise-uniform coverage plane is one filled plane (`GrayRaster::new`, `PrecisionPlane::copy` through `TilePlane::uniform_from`); `Layer.meta` is `Arc<str>`, shared by every state; `paint_mask` answers `None` when no byte changes; the typed facade metadata leases `8 ×` its inspection JSON plus the profile, not the 1 MiB `inspection_bytes` admission estimate (`PrecisionFacadeMetadata.capture`). Before: the same steps on the build before these fixes (the batch's spike); after:

| Edit (24 MP) | 8-bit before | 8-bit after | 16-bit before | 16-bit after |
| --- | --- | --- | --- | --- |
| brush dot, size 100 | 266,130 | 266,122 | 1,590,640 | 549,608 |
| `add_layer_mask reveal_all` | 24,003,986 | 157,066 | 49,066,352 | 332,416 |
| mask stroke that changes nothing | 914, one state | 0, none | 0, none | 0, none |
| mask dot, size 100 | 69,522 | 69,514 | 1,197,424 | 157,312 |
| `add_adjustment_layer levels` | 24,620,190 | 773,262 | 1,075,020 | 30,164 |
| `move_layers` (10, 10) | 1,679 | 1,552 | 1,072,464 | 27,672 |
| whole-layer invert | 96,004,751 | 96,004,624 | 193,075,536 | 192,030,744 |
| snapshot after capture / after one invert | — | 0 / 96,004,624 | — | 0 / 192,030,744 |

`python3 scripts/verify_phase13b_history.py --measure 8` (and `16`), an owned Debug instance at the 8 GiB limit. The snapshot rows are `list_snapshots.incremental_bytes`; no before was run for that sequence. **Budget decision: kept** — history at working limit / 4 and 4096 transitions, named snapshots at 32 inside limit / 4: at 24 MP the 2 GiB budget of the 16 GB minimum holds 22 whole-layer 8-bit edits or 11 at 16 bits, and dab-scale edits reach the transition cap first (4096 × 266,122 = 1.09 GB at 8 bits; 4096 × 549,608 = 2.25 GB at 16, about the budget). What limited history was the over-retention above.

**The layered RGB saves inside their plan** (2026-11-16, [#10](https://github.com/kpgalligan/doctored/issues/10)). An 8- or 16-bit PSD, PSB or layered TIFF is laid into ONE buffer (`write_psd`; `tiff_layers_write::write` takes the records and a `Strip`): the layer information appended in place, the TIFF strip streamed from the projection (16-bit) or the supplied composite (8-bit), each spot plane read a row at a time (`SpotCoverage::stored_row`). Bytes unchanged. The plan, `PsdExportReport::working_plan`: `worst_case_bytes` + `held_bytes` (the worst case less its flat picture, the pattern tail again, a PSD's spot-row headers) + a PSD's composite planes (n·4 a sample byte) + `bake_scratch_bytes` + the wide encoder; both reports state it as `working_memory_plan_bytes`, the 16-bit save admits it, `PSDExportCapture` reads it. `core/tests/layered_save_plan_memory.rs` holds each peak within it. One spot channel, `cd core && cargo run --release --example phase13b_transient_copies` (before: `flat` NULL; after: supplied, as the host does):

| Layered TIFF (24 MP) | Before: peak / plan | After: peak / plan |
| --- | --- | --- |
| 8-bit | 530,365,614 / none | 313,597,438 / 338,757,996 |
| 16-bit | 967,596,955 / 627,463,002 | 531,410,775 / 677,389,996 |

## 13B-6 contract

`core/src/cache_registry.rs`: one process-wide registry holds every resident cache payload that holds bytes — the byte and typed style planes, a byte mask's derived and aligned planes, the LUT memo's parsed tables — and the cache keeps a `Ticket` (a `Weak` plus a shared token). **Bound:** `cache_limit::registry_bound`, the working limit / 8 (the old host trim's threshold), held at every insert. **Order:** least recently used first, across classes, one clock (a hit marks use; the candidate scan and accounting do not). An evicted entry is a miss and is rebuilt; readers keep the planes they upgraded; an entry above the bound is not kept; the last ticket's drop unregisters its entry. An entry's bytes are the report's own walk over its payload. **Not members:** the content-box memos (no pixels, 512 entries each) and the contiguous views (their own 1 GiB bound); the report's process-wide `cache_registry` section lists both beside the classes. The host trim and `dr_process_cache_trim_limit` are gone; `admit` still evicts every cache and remeasures under pressure. Exit met: `core/tests/cache_registry.rs` (the bound held, the order, the mask, LUT and typed classes, class sums equal to the report's categories, an oversized entry, freed documents), each failing under a mutant. Measured at 24 MP 8-bit (the spike's session, one document opened three times): a drop-shadow entry is 48,844,344 bytes, a three-effect entry 98,496,192, a feathered mask's two entries 48,563,216, a 33³ LUT 1,030,092; the 1 GiB bound at 8 GiB holds about ten styled layers' current planes, two at the 2 GiB pin (each such layer re-renders when it falls out). The "other document's style entries leave" item that session raised is not a defect: `open_document` answers the document already open for a path (`already_open`), so each "document" was the first, and `set_layer_style` hands the old style's planes to the new one through `inherit_planes` (draining them when the pad changes); two distinct documents keep each other's entries.

## 13B-9a contract

A typed Clone or Heal (16-bit and 32-bit float, every sampling mode, every destination) is rect-first: the bound comes from the coverage alone (`RetouchKernel::sampled_frames`: the `coverage > 0` bbox, a Heal's grown by 1, clamped to the canvas), the source is read as the taps of one canvas rect (`retouch_taps16` / `retouch_taps32`; `retouch_transform::tap_rect`) and transformed over the bound only (`transform_rect*`, which walk the bound's rows whole from column 0 through `resample_rows_into`, so each kept sample is the whole-canvas transform's to the bit). `RetouchKernel::Sampled` carries its frame (`SampledPlate`); Heal runs in the frame's coordinates. The full-frame seam is `RetouchEdit::full_frame` (`#[doc(hidden)]`, not JSON), the oracle of `core/tests/precision_retouch_rect.rs`; `core/tests/rect_read_memory.rs` bounds a 100×100 dab below 2 bytes a canvas pixel. `all` / `current_and_below` still render the whole plate, and `retouch_plan` and the plate's admission are unchanged.

## Decisions

- **The 13C seam.** `Tile<S>` is opaque behind `samples()`, so a spilled or compressed tile variant lands without changing `TilePlane`.
- **Structural sharing only.** Tiles are shared by `Arc` identity between states derived from each other (and, inside one plane, among bitwise-uniform tiles of one shape: `filled`, `TilePlane::uniform_from`, `from_rows_shared_with_job`); content-addressed / interned tiles across documents are out of scope.

### 13B-2b decisions

- **The view bound is one projection's working set (1 GiB, 256 entries).** `CONTIGUOUS_CACHE_BYTES` / `CONTIGUOUS_CACHE_ENTRIES` were sized to hold a 3-layer 48 MP or 10-layer 24 MP byte document while the compositor read every byte layer's view per projection; since 13B-2c-1 it reads rows from the tiles and a projection leaves no view. A working set above the bound thrashes LRU (each read re-materializes every view); 13B-6's registry absorbs the bound and makes it working-set aware ([#10](https://github.com/kpgalligan/doctored/issues/10)).
- **The views are not a registry member** (13B-6 replaced the host trim, which excluded them for the same reason): every projection re-reads them, so they keep their own bound rather than competing with style and mask planes. They stay `cache: true`, so `EditingState`'s retained figure still subtracts them.

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

### 13B-4 decisions

- **D1 — no view cache for masks or channels.** The compositor reads an aligned plain mask one row span at a time in place (`GrayRaster::row`: a tile's own run, or the span copied into a per-call row buffer), so a projection copies no mask and leaves no view, and a dab is not re-paid as a whole-plane copy on the next read (`layer_raster_reads`' no-view exit holds for masked layers). A feathered, density-scaled or misaligned mask keeps its one cached contiguous plane per mask (13B-6's to bound).
- **D2 — the dab writers' rect is the plane's own frame.** `paint_mask` and `painting_channel` read and write the overlay's nonzero canvas rect mapped onto the plane (`layer_rect::nonzero_rect` + `canvas_rect_on_plane` over the mask's dimensions and `mask_properties.offset`; a group's canvas-sized mask; a channel's canvas), never `doc_paint::overlay_rect`'s layer frame. `paint_mask` keeps its no-latch answer (an unchanged stroke answers the mask under a fresh identity); `painting_channel` keeps NULL.
- **D3 — persistence is byte-identical with no DRDC word.** Every writer walks the plane's runs in row order or encodes one transient copy; the channel PNG's copy is the save plan's `K = c·px` term (`drdc_save_plan.rs`). The 13C pool still takes a mask or channel as the writer's bytes ([#11](https://github.com/kpgalligan/doctored/issues/11)).
- **D4 — `samples()` is a test oracle.** Both types keep a `#[doc(hidden)]` owned-copy `samples()` with no library caller; library readers use `runs` / `row_segments` / `row` / `sample_at`, or one transient `to_image` / `to_contiguous` copy for a resample input. The raw-address oracles compare `storage_identity()` (`PrecisionPlane`), `GrayRaster::ptr_eq` in the byte suites.

## Closed open items

- `DONE` — split from 13B-8 — f32 planes above `CONTIGUOUS_CACHE_BYTES` (67,108,864 px), reachable at the 8 GiB limit of a 16 GB Mac (≤ 100 MP by `native_preflight`). **Phase 1** (`5ad3dbc`): every plane built without an adopted `Vec` tiles at every size (`filled_plane`, `TilePlane::uniform_from`, the float native reopen and float TIFF open via `from_rgba32_tiles_decoded`, every `with_rect` output). **Phase 2** (`09e0b98`): `tiles_of` has no bound, so every adopted f32 plane tiles too; Contiguous is a tile-store refusal only (test seam `contiguous_rgba32_extended`). Each adoption copy is charged or removed at every size: float render finalization `F32_FINALIZATION_BYTES_PER_PIXEL` = 41 (K fitted from 40), the float reopen figure + `FLOAT_REOPEN_MARGIN_PER_PIXEL` = 9 per float raster pixel (from 8), float paint's grown arm builds into tiles, the `convert` test seam charges 2·count·bpp; the float TIFF open covers 1.45× unchanged. Measured at 24 MP and 100 MP (records: `precision_render`, `precision_native_plan`, `float_io` module docs; `tests/float_above_cache_bound.rs`).
- `DONE` — The byte `adding_layer_at` zero plane (was 13B-5's): `adding_layer` / `adding_layer_at` build through `transparent_canvas_layer` over `LayerRaster::filled`.
- `DONE` — The typed geometry plan charged no input transient (was 13B-8's): `source_copy_bytes`, `adoption_copy_bytes`, `float_source_copy_bytes` and `float_adoption_copy_bytes` (`core/src/precision_sampling.rs`) are in the plan.
- `DONE` — `evict_caches`' `scope` string (`core/src/resources.rs`) says it drops every contiguous layer view and that `removed_cache_bytes` counts only the supplied documents' views.
- `DONE` — `develop_inspection::histogram_and_full_cell_marker_scans_cancel_and_release_reservations` parks the worker (`cancelled_parked_at`) instead of racing a poller, so its load failure is gone.
- `DONE` (2026-11-12) — 13B-7's plan item: `dr_doc_render_region_plan` / `dr_precision_doc_render_region_plan` (`core/src/ffi_regional.rs`) return the plan JSON, and MCP `render` with `region` or `preview_divisor` adds `{"regional_plan": {…}}` without the `layers` rows (`AgentServer+RegionalPlan.swift`), read before the render so a refusal surfaces once. Drive: `scripts/verify_regional_plan.py` P1–P7.
- `DONE` (2026-11-12) — the regional latency item (13B-2c-1's unmeasured warm 1.06–1.10×): re-measured by 13A-latency, the per-sample tile reads cost nothing at divisor 1 (tiled / contiguous 0.98–1.01) and 1.30–1.35× on a divisor-8 preview's strided reads, the residual stated in the [13A ADR](phase-13a-regional-status.md#measured-latency).

## Verification records

Run logs, tables and receipts: the archived original and the receipts folder. A miss, gap or unrun check that still stands is an item of the status doc's [`## Open items`](phase-13b-tiled-status.md#open-items); the dropped ones keep their figures in the archived original. 13B-3e-4c-2b's above-bound probe suite is in the receipts.
