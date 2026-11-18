# Phase 13B — Tiled backing and history (ADR)

- **Status:** Accepted — the settled record as of 2026-11-17: each shipped run's contract and the decisions in force,
  condensed from the shipped companion when it reached its cap. The phase is open:
  [phase-13b-tiled-status.md](phase-13b-tiled-status.md) keeps the state, the survey, the plan, the handed items,
  `## Open items` and the runs shipped since, under [#9](https://github.com/kpgalligan/doctored/issues/9) and
  [#11](https://github.com/kpgalligan/doctored/issues/11).
- **Originals:** [archive/phase-13b-tiled-shipped.md](../archive/phase-13b-tiled-shipped.md) (the shipped companion
  through 13B-9b) and [archive/2026-10/phase-13b-tiled-status.md](../archive/2026-10/phase-13b-tiled-status.md) (the
  first status doc: every `Owns` / `Exit` cell, all 198 decisions, the verification records).
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13b/`. Files named without their path are in `core/src/`;
  a contract with no home named lives in `precision_image.rs`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Runs

35 runs. 2026-09-25: 13B-1 `c3e7a9d`, 13B-2a `6989f63`, 13B-2b `1fe7bfe`, 13B-2c-1 `5bfc798`, 13B-2c-2a `2454f6b`,
13B-2c-2b `95e09b4`, 13B-2c-2c `7add307`, 13B-2c-3a `9dc0d3d`, 13B-2c-3b `e7639f0`, 13B-3a `09c7875`, 13B-3b-1
`634746c`, 13B-3b-2 `740294b`, 13B-3c-1 `656a0aa`, 13B-3c-2a `694194c`, 13B-3c-2b `549b10e`, 13B-3c-2c-1 `71e4873`,
13B-3c-2c-2 `97e5825`, 13B-3d-1 `a33e203`. 2026-09-26: 13B-3d-2 `63421b3`, 13B-3d-3a `e250ba6`, 13B-3d-3b `2c04c47`,
13B-3e-1 `3f7e2e5`, 13B-3e-2 `a2541cf`, 13B-3e-3a `5ec96ed`, 13B-3e-3b `6b97c48`, 13B-3e-4a `277c88a`, 13B-3e-4b
`9a30000`, 13B-3e-4c-1 `31981c3`, 13B-3e-4c-2a `d1f7601`, 13B-3e-4c-2b `3a7cf21` (closed as a recorded residual).
2026-11-15: 13B-4 `79a5f644`, 13B-5 `31d4e145`…`1d3c2356`, 13B-6 `e0a5a628`. 13B-9a `d50598c2` (11-16), 13B-9b
`e7efb2fd` (11-17).

## 13B-1 contract

`TilePlane<S>` (`tile_store.rs`): 256×256 `Arc`-shared tiles for five sample types, the bitwise sharing latch,
pointer-deduplicated accounting, per-tile cancellation.

## 13B-2a contract

`LayerRaster` and `LayerRasterKey` (`layer_raster.rs`): `Layer.pixels`, `RetainedSource8.rendered` and the WARP
binding's two rasters are that type; both identity caches key on it.

## 13B-2b contract

The tiled representation, the bounded view cache (now `contiguous_views.rs`), whole-image writers through `rebuilt`,
paint through `read_rect` + `with_rect` (`layer_raster.rs`, the writer contract the typed side copies). `with_rect`
seeds no view.

## 13B-2c-1 contract

`row_segments` / `sample_at`: the byte compositor, the regional tree and `scan_content_box` read tiles in place. The
resource report is a point sample: `unique_retained_bytes` can move between two reports when another thread evicts
views.

## 13B-2c-2a contract

The rect-bounded byte writers (heal, inpaint, paint planes, bucket fill, Clear, red-eye and the rest) and
`LayerRaster::unseeded` (`layer_rect.rs`). The rect bounds the read, the diff the copies.

## 13B-2c-2b contract

Tile-aware byte equality (`same_bits`, `equals_contiguous`, `same_bytes`, `equals_image`), `to_contiguous_with_job`,
`JobContext::cancelled`. Equality takes no job.

## 13B-2c-2c contract

Every byte source producer answers a `LayerRaster`, so a RAW layer is its asset's tile plane under a fresh identity.
The report records a retained source before `layer.pixels`, so shared tiles count under `embedded_source_pixels`.

## 13B-2c-3a contract

The persistence, export, thumbnail, canvas-image, plate-copy and promotion reads off the view; the native-format and
WARP readers `unseeded`; the reopen dedup (`native_sources_byte::materialize_byte`).

## 13B-2c-3b contract

No production `contiguous_image()` call (13B-2 closed): a tap-walk input binds one transient
`to_contiguous[_with_job]` copy; the geometry outputs and the retouch temp documents are `unseeded`.

## 13B-3a contract

The typed accessor (`to_rgba16/32`, `ptr_eq`, `same_samples`; the borrowed `as_rgba16/32` removed), every reader
migrated.

## 13B-3b-1 contract

The typed flip (`Plane<S>` tiled or contiguous, an identity token), the view cache in `contiguous_views.rs`, the
handle-owned FFI lend, `plane_key()`, the tiling-transient term in `retained_source_render::work_plan`.

## 13B-3b-2 contract

The typed writers (`rebuilt_*`, `with_rect_rgba16/32`, `read_rect_rgba16/32`, `from_rgba16_unseeded`,
`reidentified`); `precision_image.rs` split into `#[path]` children.

## 13B-3c-1 contract

The typed reads (`row_segments`, `sample_at`, `runs_rgba16/32`; one u16 read rule, `normalized16`) under the typed
compositor, the regional typed tree and `image_equal`. No typed content-box scan exists.

## 13B-3c-2a contract

Every consumer of a typed projection handle on tile reads (`rgba16_at`, the quantizer, the tone map, stats, soft
proof, compare, scoped analysis), its gates, checkpoints and reports unchanged.

## 13B-3c-2b contract

The projection output unseeded; Copy / Copy Merged, the narrow and structure previews and `baked_projection` on rows
and runs.

## 13B-3c-2c-1 contract

`transient_rgba16/32` (one random-access read per operation, no view) under the resample inputs; the retouch plates
and the selection source on rows. `canvas_samples` holds one canvas-sized `W·H·8` buffer the plan does not charge
separately (decided: charging it would change admission answers).

## 13B-3c-2c-2 contract

The other 19 typed constructor sites classified; the Copy / Cut payload and `convert`'s cross-storage output
unseeded. Copy still charges `2 · payload_bytes`.

## 13B-3d-1 contract

The typed paint rect writers (paint / erase / clone / heal, the five native tools, the float Cut's erase) over the
plane forms of `layer_rect.rs`. The one canvas-proportional pass a dab has left is the bbox scan of the caller's
canvas-sized coverage.

## 13B-3d-2 contract

The in-place retouch (`precision_retouch_apply.rs`) and the lock restore (`unshared_rect`) as one-rect writers; the
whole-image writers on runs + `rebuilt_*`; an f32 destructive adjustment refuses by name. Run walks are plain nested
loops, never an iterator chain over the runs (measured up to 3.7 × slower).

## 13B-3d-3a contract

`rebuild_over` seeds nothing; the merges, stamp, flatten, bake, artwork, warp output, Rasterize, paste conversion,
retouch new layer and tone maps are unseeded. The byte `LayerRaster::new` / `rebuilt` still seed (13B-2b).

## 13B-3d-3b contract

`filled_rgba16` / `filled_rgba32_extended` and `LayerRaster::filled` (one tile per shape, no admission;
`precision_image_fill.rs`) for `add_layer_at`, the sourced shapes and the narrowing's masked plane; unseeded resample
outputs; `precision_codec::encode` over runs. A filled layer retains and admits less while the plans charge the whole
plane (a dropped 13B-8 item).

## 13B-3e-1 contract

The typed export chain, the 16-bit PSD writer and the float writers on runs, rows and one rect; every export render
and the style bake unseeded. `channels16` holds four planes (`w·h·8`) at once, inside `save_psd`'s `canvas_bytes`.

## 13B-3e-2 contract

The retained-source (RAW) render on tile reads with unseeded outputs (`color16` through one row buffer).

## 13B-3e-3a contract

The six remaining whole readers on tile reads (the warp source as one transient). An order-independent walk goes in
tile-band order, an order-dependent one row-major; a `job.every` hoisted per run keeps the flat count
(`EVERY_INTERVAL`, `job_control.rs`) but can sit 4096 + w − 1 samples apart on a contiguous plane.

## 13B-3e-3b contract

The raw develop's reads on runs; `EVERY_INTERVAL`. The develop inspection preview MISSED on tile reads and kept its
view (D4).

## 13B-3e-4a contract

The host intakes and the RAW develop output unseeded; the geometry source on one transient; the FFI lend a private
pinned copy (`Plane::lent`), NULL on an allocation failure; `dr_precision_image_storage_identity`. A lend pointer is
never an identity.

## 13B-3e-4b contract

`from_rgba16_decoded` / `from_rgba32_extended_decoded` for every typed decoder (a bitwise-uniform plane is filled,
`TilePlane::uniform_from`); the narrowing's byte planes unseeded. Reopen and PSD-open admission still charge the dense
plane (a dropped 13B-8 item).

## 13B-3e-4c-1 contract

`contiguous_rgba16/32` removed; the develop preview reads the crate-private `preview_view_rgba16`; tests use owned
copies, per-sample reads and the storage identity (`core/tests/preview_view_exception.rs` pins the view cache).
`verify_phase8d.py` drives develop inspection.

## 13B-3e-4c-2a contract

`OutputView` deleted; no f32 constructor seeds; the f32 view type deleted, so a typed view is u16 only.

## 13B-3e-4c-2b contract

Closed as a recorded residual (D1), no tiling code landed. Since the f32 tiling's phase 1 (`5ad3dbc`) every plane
built without an adopted `Vec` tiles at every size, and since its phase 2 (`09e0b98`) adopted planes tile too;
Contiguous is a tile-store refusal only.

## 13B-4 contract

Byte masks and channels (`Layer.mask`, `Channel.data`, the vector mask's raster part and baked plane, the WARP masks,
the PATH table's byte planes) are `GrayRaster` (`gray_raster.rs`), typed masks and channels `PrecisionPlane` on
`TilePlane<u16>` (`precision_mask.rs`): shared 256×256 tiles behind an identity token, no `Deref`, no `PartialEq`.
Exit met (`core/tests/mask_plane_tiles.rs`): three 300×300 dabs on a 1536×1024 plane retain the base plane plus 4
tiles per dab (4 whole planes before); the saved native bytes are the row-major samples.

Its transient copies at 24 MP, each inside the plan that admits its operation, so no plan gained a term (`cd core &&
cargo run --release --example phase13b_transient_copies`, peak heap above the baseline):

| Copy (24 MP) | Plan scratch | Peak |
| --- | --- | --- |
| byte `move_selected` mask: the stored plane's `to_image()` | 436,194,304 | 48,043,050 |
| typed `move_selected` mask: `to_contiguous()` | 484,194,304 | 96,086,807 |
| typed 0.9× resize of a masked layer: `transform_plane_outside`'s copy, `from_u16` | 736,292,840 | 693,122,082 |
| typed `set_channel`: `from_u16` holding its input beside its tiles | 100,194,304 | 48,022,228 |
| byte Levels Auto over a masked adjustment: `scoped_auto`'s effective mask | 1,323,151,872 | 528,100,738 |

`fill_align_box` (both depths) and the byte group mask thumbnail's box arm read the mask in place since 13B-9a
(`MaskCoverage::covered`; `doc_channel::placed_row`) and peak at 0; the thumbnail's arm below the box threshold still
copies the plane, bounded by that threshold.

## 13B-5 contract

What one edit retains, read from `list_history.retained_bytes` (`scripts/verify_phase13b_history.py`; `--measure
8|16` prints the 24 MP rows). Four over-retentions fixed: a bitwise-uniform coverage plane is one filled plane;
`Layer.meta` is `Arc<str>`; `paint_mask` answers `None` when no byte changes; the typed facade metadata leases `8 ×`
its inspection JSON plus the profile (`PrecisionFacadeMetadata.capture`). Before (the build before the fixes) and
after:

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

**Budget decision: kept** — history at working limit / 4 and 4096 transitions, named snapshots at 32 inside limit
/ 4: at 24 MP the 2 GiB budget of the 16 GB minimum holds 22 whole-layer 8-bit edits or 11 at 16 bits, and dab-scale
edits reach the transition cap first. What limited history was the over-retention above.

**The layered saves inside their plan** (2026-11-16, 11-17 and 11-18).
An 8- or 16-bit PSD, PSB or layered TIFF is laid into ONE buffer (`write_psd`; `tiff_layers_write::write` takes the
records and a `Strip`), the TIFF strip streamed, each spot plane read a row at a time (`SpotCoverage::stored_row`);
bytes unchanged. The plan, `PsdExportReport::working_plan`: `worst_case_bytes` + `held_bytes` + a PSD's composite
planes + `bake_scratch_bytes` + the wide encoder + `BOOKKEEPING_BYTES`; Lab adds 3·n·d for a PSD (one encoded row for
a TIFF), CMYK the larger of 68·n and `held_bytes` + 5·n·d, Gray and Duotone (`gray_save_plan`) `worst_case_bytes` +
`held_bytes` + 8·n + the stamps + the wide encoder, a Duotone file its worst case once more and a 16-bit Duotone its
8-bit copy's plan. Since 2026-11-18 the inventory charges a Gray record the two planes it writes (`export_costs`), so
`worst_case_bytes` no longer pays for the records held beside the file and the plan names them (`held_bytes`); the
32-bit Gray save admits `held_bytes` beside the 32-bit save's measured figure too (`psd_export_gray32`). Every 8- and 16-bit report states `working_memory_plan_bytes`, every such save admits it,
`PSDExportCapture` reads it; `core/tests/layered_save_plan_memory.rs` holds each peak within its plan. Layered TIFF,
one spot channel (`phase13b_transient_copies`): 8-bit 530,365,614 / none before, 313,597,438 / 338,757,996 after
(peak / plan); 16-bit 967,596,955 / 627,463,002 before, 531,410,775 / 677,389,996 after.

## 13B-6 contract

`cache_registry.rs`: one process-wide registry holds every resident cache payload that holds bytes (the byte and
typed style planes, a byte mask's derived and aligned planes, the LUT memo's parsed tables); the cache keeps a
`Ticket` (a `Weak` plus a shared token). **Bound:** `cache_limit::registry_bound`, the working limit / 8, held at
every insert. **Order:** least recently used first, across classes, one clock. An evicted entry is a miss and is
rebuilt; readers keep the planes they upgraded; an entry above the bound is not kept; the last ticket's drop
unregisters its entry. **Not members:** the content-box memos and the contiguous views (their own bound, 256 MiB); the
report's `cache_registry` section lists both. The host trim and `dr_process_cache_trim_limit` are gone; `admit` still
evicts every cache and remeasures under pressure. Exit met: `core/tests/cache_registry.rs`, each case failing under a
mutant. At 24 MP 8-bit a drop-shadow entry is 48,844,344 bytes, a three-effect entry 98,496,192, a feathered mask's
two entries 48,563,216, a 33³ LUT 1,030,092; `open_document` answers the document already open for a path, and
`set_layer_style` hands the old style's planes to the new one through `inherit_planes`.

## 13B-9a contract

A typed Clone or Heal (16-bit and 32-bit float, every sampling mode, every destination) is rect-first: the bound comes
from the coverage alone (`RetouchKernel::sampled_frames`: the `coverage > 0` bbox, a Heal's grown by 1, clamped), the
source is read as the taps of one canvas rect (`retouch_taps16` / `retouch_taps32`; `retouch_transform::tap_rect`)
and transformed over the bound only (`transform_rect*`, each kept sample the whole-canvas transform's to the bit).
`RetouchKernel::Sampled` carries its frame (`SampledPlate`); Heal runs in the frame's coordinates. The full-frame seam
is `RetouchEdit::full_frame` (`#[doc(hidden)]`, not JSON), the oracle of `core/tests/precision_retouch_rect.rs`;
`core/tests/rect_read_memory.rs` bounds a 100×100 dab below 2 bytes a canvas pixel.

## 13B-9b contract

A typed `sample: current` Spot Heal or Content-Aware Fill (16-bit, float, CMYK) reads its source layer in place
(`SynthesisPlate` carries the image and its offset; a gap reads the blank) and its plan charges no plate; the hole and
component planes cover the request's bounds. Oracle: `RetouchEdit::full_frame` (`precision_synthesis_in_place.rs`,
`cmyk_synthesis_in_place.rs`); `rect_read_memory.rs` bounds a 48×48 dab below 1 byte a canvas pixel. The byte plate
reports per tile (Tiles, Tree) in its caller's band.

## The quiet-machine residue

2026-11-18, Release builds on an M2 Max with no other build running; each sample's 1-minute load is in the batch's
receipts (3.3–6.0 for the timing rows). The timing rows are `cd core && DR_TILE_SHARING_ONLY=<group> cargo run
--release --example phase13b_tile_sharing`, one process a sample, medians of 5; base and tip alternated, every
`sha256` equal.

**The missed warm bounds**, carried through the recorded miss (the pre-tiling library is not in this history): met when
tip ÷ base ≤ bound ÷ recorded ratio. The Copy / Cut payload is built straight into tiles (no staging plane), the
flattened 16-bit PSD/PSB takes its record and image data from one walk (`flattened_planes16`), and the float native
save copies each staged chunk in one move. `copy_layer_float` 0.860 ≤ 1.018 met (`extended_pixel` written without a
branch: its check was most of the gap); `save_psd_flattened` 0.945 ≤ 0.970 met; `save_native_float` 0.803 ≤ 0.823
met. `copy_layer` (u16; at 3000×2000, since its host stage refuses 24 MP at the 2 GiB pin) 1.023 against ≤ 0.951:
MISSED, its bound restated at **1.16 ×** (1.157 × 1.023, capped at the recorded miss). Cause, measured in one binary
(`DR_TILE_SHARING_ONLY=copy_split`: the stages alternated in one process, medians of 9 rounds, five processes at load
3.5): the warm Copy's 18.4 ms is the payload build, 6.4, and the RZCB encode, 11.5 (a 2-byte append a sample). The
same build with no tile on either side takes 0.9: on tiles the source's `row_segments` read (one run a tile column)
takes 2.9 against 0.8 from one contiguous buffer, and the rest of the build the other 3.5 (the tile write and its
per-row checks). That tile layout's 5.5 ms
(30 % of the call) covers the miss (14 % of it). The staging copy is not the cause (removing it moved the median
−4 % to +4 % over nine alternated sets); the change is kept for its memory: the whole-layer Copy's peak is its
payload, 48,029,392 for 48,000,000 at 16 bits (96,005,480 before, `make t F=rect_read_memory T=a_whole_layer_copy
A=--nocapture`). Since a later 2026-11-18 batch the encode appends each run's bytes in one move on a little-endian
target (`float_native::put_samples`'s pattern), `ms_encode` 11.56 → 2.10 and the warm `copy_layer` 18.72 → 10.42 ms
(`copy_split` and `clipboard`, base and tip alternated, medians of 5, 1-minute load 47.0 at the start, 14.3 to 15.1
later, after a build, every `sha256` equal); the 1.16 × bound stands.

**Typed latency bounds** (`latency`; 1.10 × the median, rounded up to 0.1 ms, `LATENCY_BOUNDS_MS`): `typed/dab` 97.6 ms
the 8 dabs / 11.8 ms the median dab, `typed/dab_blend` 98.4 / 12.4, `typed/projection` 399.2 cold / 344.0 warm,
`typed/save` 219.5, `typed/projection_masked` 465.6 / 427.2, `regional_window` 24.5 / 22.9, `regional_window_masked`
29.7 / 29.2; the run after printed `MET` on every line. A canvas mask costs the typed projection 3.25 ns a mask
sample (1.25 ×, masked warm 388.5 ms) and the byte regional window 5.34 ns (1.27 ×, 26.5 ms), each mask sample one
`TilePlane::sample_at` lookup (2026-11-18, the same `latency` command, base and a cursor build alternated, medians of
5, 1-minute load 15.3 at the start, 8.1 to 11.3 later). A one-tile cursor in its place (each read's owner keeping
the last tile's `Arc` and origin, bit-equal to the lookup across tile seams) cost more on the typed read, 3.85 ns
(1.30 ×, 400.9 ms, the five-run ranges apart), and was not faster on the byte read, 5.85 ns (1.29 ×, 27.2 ms, the
ranges overlapping), so the per-sample lookup stays and no bound moved.

**The view cap is 256 MiB** (`views`, declared: stay 1 GiB if a 48 MP u16 miss exceeds 50 ms): a miss costs 7.7 ms
(24 MP 8-bit, 96 MB), 17.5 (48 MP 8-bit), 16.2 (24 MP u16) and 31.5 (48 MP u16, 384 MB); a hit 0.0002. A 10-layer 24
MP byte document's seeds held 960,000,000 bytes at 1 GiB, 192,000,000 at 256 MiB.

**Release memory drives**, plan ≥ ⌈1.25 × max E⌉, three runs and a twin each, all PASS. CMYK retouch
(`DR_VERIFY_APP=build/Build/Products/Release/Doctored.app python3 -B scripts/cmyk-admission/measure.py --runs 3`):
`dodge-16` 605,924,864 against max E 299,729,136, `smudge-16` 657,695,832 / 301,777,112, `history-brush-16`
621,420,032 / 42,254,312, `blur-sal-16` 7,468,608,928 / 2,279,360,336, `smudge-sal-16` 3,951,631,368 /
1,398,080,712, `retouch-clone-16` 620,606,424 / 14,663,704. Duotone (`scripts/duotone-admission/measure.py`, all ten
cases): `export-duotone-psd16` 1,582,538,480 / 52,428,824, the tightest `set-duotone` 745,048,576 / 576,422,296. The
float retouch rows are `float_retouch`'s table (`examples/float_retouch_admission.rs`, now over the call's own
footprint interval: the boxed cases' whole-run peak is the document build's).

## Decisions

- **The 13C seam.** `Tile<S>` is opaque behind `samples()`, so a spilled or compressed tile variant lands without
  changing `TilePlane`.
- **Structural sharing only.** Tiles are shared by `Arc` identity between states derived from each other (and, inside
  one plane, among bitwise-uniform tiles of one shape); content-addressed tiles across documents are out of scope.

### 13B-2b decisions

- **The view bound was one projection's working set (1 GiB, 256 entries)**, `CONTIGUOUS_CACHE_BYTES` /
  `CONTIGUOUS_CACHE_ENTRIES`; since 13B-2c-1 a projection reads rows from the tiles and leaves no view, so 13B-6's
  decision (2026-11-18) is 256 MiB: the readers left are the develop preview, the cropped delete, the crop's padded
  background and the byte writers' seeds, and a 48 MP u16 miss costs 31.5 ms (the quiet-machine residue above).
- **The views are not a registry member**: every projection re-reads them, so they keep their own bound. They stay
  `cache: true`, so `EditingState`'s retained figure still subtracts them.

### 13B-2c-2a decisions

- **Inpaint narrowing.** A fill whose union window misses the layer answers `Ok(None)` before any solve
  (`doc_inpaint.rs`), so the solve's errors are no longer raised for a call that cannot move a byte; the checks ahead
  of the loop keep their order, and a post-plan reserve charged by the union rect follows them.

### 13B-3b-1 decisions

- **(c) Planes above `CONTIGUOUS_CACHE_BYTES` stayed Contiguous** because the cache would refuse their view; since
  the f32 tiling's phase 2 (`09e0b98`) every plane tiles, each plan charging its copy. The view bound itself is 13B-6's:
  256 MiB since 2026-11-18.

### 13B-3c-2c-2 decisions

The 19-site Seeded / Unseeded classification is in the first archived original; the one kept preview seed
(`raw_source_geometry.rs`) is the only seeded typed constructor site in `core/src`.

### 13B-3e-3b decisions

- **D1 — the geometry source bind** takes ONE transient, done by 13B-3e-4a once the host intakes were unseeded;
  per-tap `rgba16_at` reads are declined (a tap walk takes one transient, 13B-2c-3b).
- **D4 — the preview MISS and its revert.** `PrecisionImage::develop_preview` (`develop_inspect.rs`) stays a
  whole-view read through `preview_view_rgba16`, and the geometry output (`raw_source_geometry.rs`) stays the seeded
  `from_rgba16`, which the app feeds to the preview (`RawDevelopPreviewSession.swift`). On tile reads the preview
  measured `preview_fit` 1.338 ×, `preview_fit_clipping` 1.057 × and `preview_detail` 1.051 × against ≤ 1.05 ×.

### 13B-3e-4c-1 decisions

- **D2 — the develop preview keeps a narrow crate-private view-backed accessor**, the removed `contiguous_rgba16`'s
  body verbatim, so every answer, refusal, report and native-format byte stayed what it was.

### 13B-3e-4c-2b decisions

- **D1 — closed as a recorded residual.** A float document cannot hold an f32 raster above 1 GiB (67,108,864 px, the
  view bound then) at the 2 GiB pin: `native_preflight`'s float `finish_plan` (`precision_native_plan.rs`) charges
  `reopen_retained` ≥ D + 1 MiB plus `decode_scratch_bytes` ≥ D + 16·n + 1 MiB, so every document raster was inside
  `tiles_of`'s bound; the f32 tiling (`5ad3dbc`, `09e0b98`) since tiles every f32 plane at every size.
- **D5 — the typed geometry plan charged no input transient**: closed (Closed open items below).

### 13B-4 decisions

- **D1 — no view cache for masks or channels.** The compositor reads an aligned plain mask one row span at a time in
  place (`GrayRaster::row`); a feathered, density-scaled or misaligned mask keeps one cached plane (13B-6's to bound).
- **D2 — the dab writers' rect is the plane's own frame.** `paint_mask` and `painting_channel` read and write the
  overlay's nonzero canvas rect mapped onto the plane (`layer_rect::nonzero_rect` + `canvas_rect_on_plane`).
- **D3 — persistence is byte-identical with no DRDC word.** The 13C pool still takes a mask or channel as the
  writer's bytes ([#11](https://github.com/kpgalligan/doctored/issues/11)).
- **D4 — `samples()` is a test oracle** (`#[doc(hidden)]`, no library caller); library readers use `runs` /
  `row_segments` / `row` / `sample_at`, or one transient copy for a resample input.

## Closed open items

- `DONE` — f32 planes above `CONTIGUOUS_CACHE_BYTES` tile at every size (phase 1 `5ad3dbc`, phase 2 `09e0b98`), each
  adoption copy charged or removed (float render finalization 41 B a pixel, the float reopen margin 9); records in the
  `precision_render`, `precision_native_plan` and `float_io` module docs; `tests/float_above_cache_bound.rs`.
- `DONE` — the byte `adding_layer_at` zero plane: built through `transparent_canvas_layer` over `LayerRaster::filled`.
- `DONE` — the typed geometry plan charges its input transient (`precision_sampling.rs`'s copy and adoption terms).
- `DONE` — `evict_caches`' `scope` string (`resources.rs`) states what it drops and counts.
- `DONE` — `develop_inspection`'s cancel test parks the worker (`cancelled_parked_at`).
- `DONE` (2026-11-12) — 13B-7's plan item: `dr_doc_render_region_plan` / `dr_precision_doc_render_region_plan`
  (`ffi_regional.rs`) and MCP `render`'s `regional_plan` (`AgentServer+RegionalPlan.swift`); drive
  `scripts/verify_regional_plan.py` P1–P7.
- `DONE` (2026-11-12) — the regional latency item, re-measured by 13A-latency: 0.98–1.01 at divisor 1, 1.30–1.35× on
  a divisor-8 preview ([13A ADR](phase-13a-regional-status.md#measured-latency)).

## Verification records

Run logs, tables and receipts: the archived originals and the receipts folder. A miss, gap or unrun check that still
stands is an item of the status doc's [`## Open items`](phase-13b-tiled-status.md#open-items).
