# Phase 13B — Tiled backing and history: status

Section **13B** of the [implementation plan](professional-photo-editor-implementation-plan.md)
("## 13. Scale to large production documents"): move retained pixel storage and
undo to shared tiles, preserving immutable-document semantics; reuse unchanged
tiles and bound resident caches across documents/jobs. 13B is split into the
bounded runs below, delivered in series; this document is the plan every later
run picks up and the record of each run.

**13B-1 is Implemented 2026-09-25** (the shared immutable tile store and its
accounting, core-only). **13B-2a is Implemented 2026-09-25** (`LayerRaster`,
the byte layer-pixel accessor, with every reader through it; representation
unchanged). **13B-2b is Implemented 2026-09-25** (byte layer pixels on
`TilePlane<[u8;4]>` tiles with a bounded process-wide contiguous-view cache).
**13B-2c-1 is Implemented 2026-09-25** (the compositor, the regional
per-sample tree and the content-box scan read layer pixels straight from the
tiles). **13B-2c-2a is Implemented 2026-09-25** (the rect-bounded byte
writers read and write only their rect; `rendered_group`'s synthetic no
longer seeds the view cache). **13B-2c-2b is Implemented 2026-09-25**
(job control on the conversion's whole-layer copy; tile-aware bytes equality
at the five comparison sites). **13B-2c-2c is Implemented 2026-09-25**
(asset-side tiles: every byte source asset and every rendered source is a
tiled `LayerRaster`, so a placed layer IS its asset's tile plane and counts
once, before and after its first edit). **13B-2c-3a is Implemented
2026-09-25** (the persistence / export / preservation / thumbnail /
canvas-image / sampling-plate / promotion reads go through the tiles, so a
save, an export, a thumbnail or a reopen leaves no view; a reopened placed
layer shares its asset's tiles again). **13B-2c-3b is Implemented
2026-09-25** (the resample-input and temp-document reads leave no view of
their input; the `contiguous_image()` residual is 0 and **13B-2 is
closed**). **13B-3a is Implemented 2026-09-25** (the typed accessor:
`PrecisionImage`'s bind-once `contiguous_rgba16/32` views, `to_rgba16/32`,
`ptr_eq` and `same_samples`, with the borrowed `as_rgba16/32` removed and
every reader migrated; representation unchanged). **13B-3b-1 is
Implemented 2026-09-25** (typed `PrecisionImage` samples on
`TilePlane<[u16;4]>` / `<[f32;4]>` tiles with a contiguous fallback, served
by the ONE process-wide view cache now shared with the byte side
(`contiguous_views.rs`); the two FFI pointer exports lend a handle-pinned
view; the tiling transient planned in `work_plan`). **13B-3b-2 is
Implemented 2026-09-25** (the typed writers: whole-image typed edits share
every unchanged tile through `rebuilt_rgba16` / `rebuilt_rgba32_extended`,
`with_rect` / `read_rect` land with their suite, the 16-bit structure
promotion seeds no view, a typed identity placement shares its master's
tiles; `precision_image.rs` split into two `#[path]` children).
**13B-3c-1 is Implemented 2026-09-25** (the typed read primitives
`row_segments` / `sample_at` / `runs_rgba16/32` on `PrecisionImage`; the
typed compositor, the regional typed tree and `precision_geometry`'s polled
comparison read straight from the tiles — a typed projection or regional
window leaves no view over the document (0, as at HEAD: since 13B-3b-1 the
compositor's `pixel(index)` already read the tiles, so 13B-3c-1's gain is
time); residual 72 → 69 call lines). **13B-3c-2a is Implemented
2026-09-25** (every consumer of a typed projection handle — quantize /
preview, `tone_mapped_u16` / the float display, the u16 and float stats,
the typed soft proof, the high-depth compare and the scoped typed analysis
— reads the tiles through `row_segments` / `runs_rgba16/32` / `sample_at`
and the new `rgba16_at`, so none binds a view; outputs, checkpoints and
reports byte-identical; residual 69 → 59 call lines). **13B-3c-2b is
Implemented 2026-09-25** (every typed render names its `OutputView`: the
host projection, the regional window and every internal projection read
only through tile reads — Copy / Copy Merged, both conversion previews, the
bake's opacity scan, the quantize / tone-map conversions, the scoped
analysis source — are `Unseeded`, the layer-producing and whole-binding
renders stay `Seeded` with their readers named; a 24 MP u16 projection, a
3000×2000 float projection and a 1024×1024 window leave 0 view bytes, were
384000000 / 192000000 / 16777216; outputs, checkpoints and reports
byte-identical; residual 59 → 53 call lines). **13B-3c-2c-1 is Implemented
2026-09-25** (13B-3c-2c SPLIT at its spec: the retouch plates, the selection
source and the typed resample inputs leave no view — the Current plate
copies the layer's rows, the cut plate and the selection source render
`Unseeded`, the plate readers read rows or ONE transient copy
(`transient_rgba16/32`), `precision_sampling`'s three binds are transient
copies; `tone_mapped_u16` takes an `OutputView`; a held 24 MP plate or
transformed plate leaves 0 view bytes, was 192000000–384000000; outputs,
checkpoints and reports byte-identical; residual 53 → 45 call lines).
**13B-3c-2c-2 is Implemented 2026-09-25** (the other 19 typed constructor
sites classified Seeded / Unseeded with their readers and flip owners named;
the Copy / Cut payload and `convert`'s cross-storage output are built
unseeded — `from_rgba32_extended_unseeded` joins `from_rgba16_unseeded` —
and the payload's two sequential readers, the RZCB `encode` and `converted`,
walk `runs_rgba16/32`; a held 24 MP Copy / Copy Merged handle and a held
3000×2000 float Copy handle leave 0 view bytes, were 192000000 / 96000000;
RZCB streams, outputs, checkpoints and reports byte-identical; residual
45 → 41 call lines; the 9g probe reader peels the 12E-1 identity word, so
`verify_phase9g.py` runs again). **13B-3d-1 is Implemented 2026-09-25**
(13B-3d SPLIT at its spec: the typed PAINT writers — 7D paint / erase /
clone / heal, the five native tools and the float Cut's erase — read and
write only the rect their coverage or window can change through
`read_rect_rgba16/32` + `with_rect_rgba16/32`, so a typed dab keeps ≤ 4 new
tiles, walks no whole plane and leaves no view; the 24 MP 8-dab history
holds only the base's own seed (192000000 view bytes, were 960000000) and a
dab is ~10 × faster; every answer byte-identical; residual 41 → 36 call
lines). **13B-3d-2 is Implemented 2026-09-26** (the in-place retouch arm —
the app's 16-bit Clone Stamp / Healing Brush / Spot Heal / Content-Aware
Fill / red-eye — and the transparency-lock restore read and write ONE rect,
so a retouch dab and a locked dab keep ≤ 4 new tiles and leave no view; the
whole-image writers read runs and answer through `rebuilt_*`, the
destructive adjustment through the new `rebuilt_float_samples`; the 24 MP
8-dab `typed/retouch_dab` / `typed/locked_dab` hold only the base's own
seed (192000000 view bytes, were 960000000 / 768000000) at 0.77 × / 0.14 ×
HEAD's dab time; every answer byte-identical; residual 36 → 24 call
lines). **13B-3d-3a is Implemented 2026-09-26** (13B-3d-3 SPLIT at its
spec: every typed LAYER producer that flips through an existing unseeded
constructor or the `rebuilt_*` tiling policy leaves no view — the merges,
stamp, flatten and members render, the bake, the artwork render, the warp
output, Rasterize, the converted paste, the retouch new layer,
`float_to_u16`, the u16 narrowing and every `rebuilt_*` answer; the 24 MP
`typed/producers` lines hold 0 view bytes, were 192000000 (384000000 for
the bake), the warp only its source's 48000000, was 96064016; every answer,
checkpoint and report byte-identical; warm times ≤ 1.04 × HEAD; the seeded
constructor / `OutputView::Seeded` sites 47 → 35 lines, residual unchanged
at 24 call lines). **13B-3d-3b is Implemented 2026-09-26** (the FILLED
planes — `add_layer_at`'s leaf, the promotion plan's sourced shapes and the
narrowing's masked-adjustment plane — are one tile per tile shape over
`TilePlane::filled`, no view: a 24 MP u16 added layer's plane retains
1227776 bytes, was 192003072, and holds 0 view bytes, was 192000000; the
resample outputs take their caller's `OutputView` — the geometry outputs,
the placed source's Sample arm and `develop_detail` Unseeded, only the
export resize Seeded — so the 24 MP geometry lines hold 0 view bytes; and
`precision_codec::encode` walks runs, so the first native save / PNG16 /
TIFF16 after a flatten binds no view, `save_after_flatten` `views_held`
192000000 → 0; every answer, checkpoint, report and `.rz` / PNG / TIFF byte
unchanged; residual 24 → 23 call lines, the seeded-site count 35 → 33).
**13B-3e-1 is Implemented 2026-09-26** (13B-3e SPLIT into four runs: the
typed export chain, the 16-bit PSD writer and the float TIFF / float native
writers read tiles — runs, rows, one rect — and every export render, export
constructor and the style bake stop seeding, so no typed export, PSD save or
float save leaves a view: the `typed/exports` lines hold 0 view bytes, were
192000000 (24 MP TIFF16 P3 export, flattened PSD), 384000000 (24 MP layered
PSD with a baked style), 48000000 (3000×2000 sharpened PNG16) and 96000000
(3000×2000 float TIFF32 / native save); every output byte, refusal,
checkpoint and report unchanged; residual 23 → 15 call lines, the
seeded-site count 33 → 24; eight of ten warm lines within ≤ 1.05 × HEAD, the
flattened PSD 1.08 × and the float native save 1.28 × MISSED, recorded with
their load). **13B-3e-2 is Implemented 2026-09-26** (the typed live filter
and the retained-source render read tiles — `source16` hands the coloured
plane's runs to the filter, `color16` walks the master's rows through one
row buffer — and the promoted byte master, `color16`'s output and `render8`'s
quantized 16-bit render stop seeding; the 16-bit PREVIEW's tile row cursor
MISSED the warm bound (1.20 × HEAD after one fix pass), so the preview read
and the filtered plane's seed REVERTED to HEAD's: the `typed/live_filter`
typed edit lines hold the one seeded filtered plane, 48000000 view bytes
(were 96000000 / 96000000 / 48000000), the byte-canvas edit and the
unfiltered P3 preview 0 (were 72000000 / 24000000); every answer,
checkpoint and report unchanged; residual 15 → 13 call lines, the
seeded-site count 24 → 22; all six warm lines ≤ 1.05 × HEAD after the
revert). **13B-3e-3a is Implemented 2026-09-26** (13B-3e-3 SPLIT at its
spec: the six remaining whole readers of an installed typed layer or caller
image — the selected Pixelate and automatic tone, the RGBA plane read, the
refinement cutout's scan, the WARP encode and the warp's source read — read
tiles (TILE-BAND where order-independent, runs row-major where not, ONE
transient for the warp's taps), and the scalar filter's gray image is built
unseeded; every `typed/whole_readers` line holds 0 view bytes (were
192000000 at 24 MP / 48000000 at 6 MP) with every answer, checkpoint and
report unchanged; residual 13 → 7 call lines, the seeded-site count 22 →
20; all eight warm lines ≤ 1.05 × HEAD after the D7 fix pass; the stale 9b
probe expectation repaired). **13B-3e-3b is Implemented 2026-09-26** (the
raw develop's variant and base reads walk tile runs, so a develop holds
only its output's seed — `typed/raw_develop` develop lines 192000000 view
bytes, were 384000000 / 576000000 — with every answer, refusal,
checkpoint and report unchanged; the develop inspection PREVIEW on tile
reads MISSED its warm bound after one fix pass (1.34 × / 1.06 × / 1.05 ×
HEAD), so `develop_inspect.rs:136` and its feeding seed
`raw_source_geometry.rs:466` KEPT HEAD's; the geometry source bind
`raw_source_geometry.rs:392` and the develop output seed
`raw_local_develop.rs:264` deferred to 13B-3e-4 with the host-intake
seeds; residual 7 → 5 call lines, the seeded-site count 20 → 20; all
seven warm lines ≤ 1.05 × HEAD after the revert; `EVERY_INTERVAL` named;
the stale 9f probe expectation repaired). **13B-3e-4a is Implemented
2026-09-26** (13B-3e-4 SPLIT at its spec into 4a / 4b / 4c: the two host
intakes and the RAW develop output are built unseeded — `typed/raw_intake`
intake lines 0 view bytes, were 192000000; `typed/raw_develop` develop
lines 0, geometry lines 192000000, were 384000000 — the RAW source geometry
reads ONE transient, and the FFI lend pins a PRIVATE copy with no cache
access, NULL on an allocation failure; the storage identity
`rz_precision_image_storage_identity` is exported for the 9h Move probe's
no-copy oracle; all 128 record lines identical to HEAD `6b97c48`; residual
5 → 2 call lines, `rg -c` 7 → 4; the seeded-site count 20 → 17; every
bounded warm line ≤ 1.05 × HEAD). **13B-3e-4b is Implemented 2026-09-26**
(every reader-side typed decoder — the typed `.rz` layer / master PNG, the
WARP original, the f32 `.rz` raster, the PNG16 / TIFF16 / float TIFF open,
the 16-bit PSD layer and composite, the RZCB / RGBA8 clipboard payloads —
builds through `from_rgba16_decoded` / `from_rgba32_extended_decoded`,
seeding nothing, and a bitwise-uniform decoded plane — the reopened empty
layer, a pixel-less PSD record — is ONE filled plane; the narrowing's byte
planes are unseeded; `OutputView::Seeded` and the two seeded render
constructors are removed; `typed/decoders` lines 0 view bytes, were
96000000–576000000, the two preview lines `w·h·8`; `typed/filled`
`narrow_masked_adjustment` 0, was 96000000; all 72 `decode` record lines
identical to HEAD `277c88a`; every peak line tree ≤ HEAD; the seeded-site
count 17 → 2; `rg -c` still 4; all 11 warm lines ≤ 1.05 × HEAD).
**13B-3e-4c-1 is Implemented 2026-09-26** (13B-3e-4c SPLIT at its spec
into 4c-1 / 4c-2: the two interactive previews — the 16-bit live-filter
preview and the develop inspection preview — read the ONE crate-private
`preview_view_rgba16`, the kept exception owned by 13A-latency, with no
new tile-read attempt after the 13B-3e-2 and 13B-3e-3b MISSes;
`PrecisionImage::contiguous_rgba16/32` and `View::into_rgba32` are
removed; 421 test / example calls (406 lines, 81 test files and the
example) read owned copies, per-sample reads or the storage identity; the
new `preview_view_exception` suite pins the view-cache contract through the
one remaining reader and passes at HEAD `9a30000`; every record and
`views` line identical to HEAD, every peak line equal; residual 2 → 0 call
lines, `rg -c` 4 → 0; the seeded-site count stays 2).
**13B-3e-4c-2a is Implemented 2026-09-26** (13B-3e-4c-2 SPLIT at its spec
into 4c-2a / 4c-2b: `OutputView` and its nine `view` parameters are
deleted — renders, tone maps and resamples take no output-view argument and
are always unseeded — and no f32 constructor seeds the view cache any more:
`from_rgba32` / `from_rgba32_extended` tile unseeded, `tile()` seeds only a
contiguous u16 plane, `View::Rgba32` / `PlaneKey::Rgba32` and the
`from_rgba32_extended_unseeded` alias are gone; the new
`precision_f32_unseeded` suite fails at HEAD `31981c3` only on its
`no_view_*` test, by design; all 634 record lines and the 72-line
`phase7_precision` output identical to HEAD; the three f32 `views` lines
4992000 → 0 bytes, every peak line tree ≤ HEAD; `OutputView` mentions 71
lines → 0; `precision_image_build.rs` 616 → 594 lines, carried nit 5
closed; above-bound f32 tiling is 13B-3e-4c-2b's).
**13B-3e-4c-2b is Closed as a recorded residual 2026-09-26** (no tiling
code lands: a float document's native plan admits f32 rasters only below
~2^30 − 1 MiB in total, so no admitted document path retains an f32 plane
above `CONTIGUOUS_CACHE_BYTES`; measured: the 8200×8193 all-0.0 float TIFF
open is refused with "needs 2151953720 bytes … working limit is
2147483648", identically at HEAD `d1f7601` and with the tiling code; the
tiling was implemented, then backed out by user decision, and the question
moves to 13B-8; carried nits 1–3 are closed).
13B-4 … 13B-9 are planned.
13B is NOT closed.

## Survey — retained storage and undo today (before 13B-1)

One row per fact, with file:line evidence as of HEAD `576f69f`.

| Area | Fact | Evidence |
| --- | --- | --- |
| Byte layer pixels | `Layer.pixels: Arc<RgbaImage>` — one contiguous canvas-or-layer-sized RGBA8 buffer per layer | `core/src/doc.rs:96` |
| Byte layer mask | `mask: Option<Arc<GrayImage>>` — one contiguous u8 plane | `core/src/doc.rs:115` |
| Byte document | `RzDocument` holds `layers: Vec<Layer>` plus `Arc`-shared `profile` / `linear_light` | `core/src/doc.rs:220-270` |
| Byte pixel edits | whole-buffer replace `with_layer_pixels`, clone-then-mutate `with_layer` — an edit of any size replaces the layer's whole buffer | `core/src/doc.rs:985-1005`, `:1005-1016` |
| Byte channels | `Channel.data: Arc<GrayImage>`, canvas-sized | `core/src/doc_channel.rs:47` |
| Typed rasters | `Pixels::U16(Arc<Vec<[u16;4]>>)` / `F32(Arc<Vec<[f32;4]>>)` | `core/src/precision_image.rs:34-37` |
| Typed working cap | `MAX_WORKING_BYTES` = 2 GiB | `core/src/precision_image.rs:15` |
| Typed accounting | capacity-based `storage_bytes`, `retained_bytes` through `Inventory` | `core/src/precision_image.rs:232-242` |
| Typed document | `Content::Raster { image }` / `Group(Arc<Vec<_>>)`; `layers: Arc<Vec<_>>`; `channels: Arc<Vec<_>>` | `core/src/precision_document.rs:29-36`, `:378`, `:380` |
| Typed masks/channels | `PrecisionPlane { pixels: Arc<Vec<u16>> }`; `PrecisionMask`; `PrecisionChannel` | `core/src/precision_mask.rs:18-21`, `:105`, `:266` |
| Dedup accounting | `Inventory::record` keys allocations by pointer, so a buffer shared by many states counts once | `core/src/resources.rs:55-73` |
| Document walk | `layer_pixels` / `layer_masks` / `saved_channels` categories | `core/src/resources.rs:105-160` |
| Report keys | `unique_retained_bytes`, `referenced_bytes`, `shared_bytes_saved`, `categories` | `core/src/resources.rs:165-223` |
| Cache inventory / eviction | `cache_inventory`, `evict_caches` | `core/src/resources.rs:225-246`, `:341` |
| Clone | `document_clone.rs` shares every `Arc`, copies strings | `core/src/document_clone.rs:1-3`, `:96-174` |
| Projection scratch | conservative 20 B/px canvas estimate | `core/src/render_scratch.rs:14-16` |
| Style cache | `CACHE_CAPACITY = 4` planes per `Arc<LayerStyle>`, `Weak` keys, bytes unbounded | `core/src/style_cache.rs:125` |
| Mask caches | derived/aligned `Arc<Mutex<Option<_>>>` per layer, unbounded | `core/src/doc_mask.rs:44-45`, `:144`, `:160` |
| LUT cache | 64 MiB cube bound | `core/src/adjust_lut.rs:73` |
| Content-box memo | 512 entries | `core/src/doc_group_query.rs:267` |
| Live-filter cache | 128 MiB | `core/src/live_filter_cache.rs:26` |
| Native writer | reads contiguous `as_raw()` (format untouched by 13B-1) | `core/src/rzdc.rs:415-417`, `:435`, `:493` |
| Unrelated tiles | 13A-5c-2b's render-only plate tiles (≤ 1024×64) in `doc_regional_plate.rs` and `tiled_soft_matting.rs`'s 256 px solve tiles are render/solve scratch, not retained storage | `core/src/doc_regional_plate.rs`, `core/src/tiled_soft_matting.rs:19` |
| Swift undo | `levelsOfUndo = 0` — retention is bounded by memory, not a revision count | `app/Sources/ImageDocument.swift:218` |
| History budget | `min(workingLimit/2, 512 MiB)` incremental bytes; ≤ 4096 transitions | `app/Sources/DocumentHistory.swift:25`, `:27`, `:34-36` |
| History measurement | `measure()` → `EditingState.retainedBytes(in:)` → `RasterDocument.resourceReport` over ALL states' handles in one call — `Arc` dedup across states is what makes history sharing free once buffers are tiles | `app/Sources/DocumentHistory.swift:107-116`, `app/Sources/EditingState.swift:30-33` |
| History eviction | `enforceBudget` drops oldest states / redo tails while over budget | `app/Sources/DocumentHistory.swift:121-141` |
| Named snapshots | 32 max, `min(workingLimit/4, 512 MiB)` | `app/Sources/NamedSnapshots.swift:35-36` |
| Working limit | `min(RAM/3, 2 GiB)`; `report` `:65`, `admit` `:139` | `app/Sources/RasterResources.swift:21` |
| MCP | `resource_usage` | `app/Sources/AgentServer+Jobs.swift:23-31` |
| FFI | `rz_resources_report` `:51`, mixed `:90`, `_with_raw_masks` `:137`, evict `:193` | `core/src/ffi_resources.rs` |

**Migration blast radius** (recorded for 13B-2 … 13B-4, counted with `grep` at
HEAD `576f69f`): 452 `.pixels` sites in 109 `core/src` files; 103 `as_raw()`
sites; 52 files reading `as_rgba16` / `as_rgba32`.

**Re-counted for 13B-2a** at HEAD `c3e7a9d` (`git grep -P
'\.pixels\b|\.rendered\b|source_pixels|rendered_pixels'`): 112 `core/src` /
100 `core/tests` files, 468 src / 604 test `.pixels` occurrences of every kind
(`RzImage.pixels`, typed and PSD records included). The compile-driven
migration touched exactly the `Layer.pixels` / `RetainedSource8.rendered` /
`WarpBinding8` share: 52 `core/src` files edited (+ the new
`layer_raster.rs`; 150 lines removed, 207 added) and 59 `core/tests` files
edited (+ the new `layer_raster.rs` suite; 311 removed, 571 added). The
tracked residual after 2a is **44 `contiguous_image()` calls in `core/src`**
(the scan counts 46: two are comments, `layer_raster.rs:22` and
`doc_regional_tree.rs:160`).

## Sub-slice plan

In dependency order. Each is one bounded implementer run with its own
verification; a run may be SPLIT at its own spec.

| Run | Owns | Exit |
| --- | --- | --- |
| **13B-1** | `core/src/tile_store.rs`: `TilePlane<S>` over the five retained sample types, bitwise sharing latch, pointer-deduplicated `accounting` / `record_resources`, per-tile cancellation; the measurement example; this plan and survey. Core-only. | Every existing test green; tile_store suite (geometry, round trips, sharing, latch, parked cancels, `visited + 2` counts) green; `phase13b_tile_sharing` prints `unique_tiles` 416 / `referenced_tiles` 3456 for the 24 MP dab sequence. **Implemented 2026-09-25.** |
| **13B-2a** | `core/src/layer_raster.rs`: `LayerRaster` (private representation, still the exact `Arc<RgbaImage>`) and `LayerRasterKey`; `Layer.pixels`, `RetainedSource8.rendered`, `WarpBinding8`'s `source_pixels` / `rendered_pixels` flipped to it; both identity caches keyed on `LayerRasterKey`; every reader migrated through `contiguous_image` (bound once) / `to_contiguous` / `ptr_eq`; the black-box contract suite. | Every existing test green; every pixel output, `.rz`/PSD byte and report unchanged; `layer_raster` suite green. **Implemented 2026-09-25.** |
| **13B-2b** | The `TilePlane<[u8;4]>` representation inside `layer_raster.rs` (`shared` stays contiguous until its first write); the bounded process-wide `contiguous_image` view cache (1 GiB / 256 entries, LRU, one entry per plane, `contiguous_views` report category, evicted by `rz_resources_evict_caches`, excluded from the host trim); whole-image writers through `rebuilt` and the two paint writers through `read_rect` + `with_rect` over the overlay's nonzero rect; `covers` as the first `read_rect` caller; the three carried nits; `phase13b_tile_sharing` `document_dab` / `flattened`. | Every existing test green, `.rz`/PSD byte-identical, a dab on a real document retains ≤ 4 tiles (416 allocations / 104388608 bytes for the 24 MP 8-dab history). **Implemented 2026-09-25.** |
| **13B-2c-1** | Compositor / regional / content-box reads straight from tiles: `TilePlane::row_segments` / `sample_at` and the `LayerRaster` wrappers; the byte compositor (`composite_layer_into`, `composite_pixels_into`, the knockout unit pass, `Shape::of_layer`, the cutout's opaque precondition) reading rows through `row_segments`; the regional per-sample tree and `scan_content_box` through `sample_at` (`ResolvedNode.pixels` removed); the `phase13b_tile_sharing` `regional_window` op; the carried nits 1–3 (`contiguous_views_of` split, one eviction walk, the eviction scope string, the report's point-sample statement). | Every existing test green, pixel output / `.rz` / PSD bytes / report keys unchanged, a projection and a regional window leave 0 views, residual 31 call lines. **Implemented 2026-09-25.** |
| **13B-2c-2a** | The rect-bounded byte writers through `read_rect` + `with_rect` (`layer_rect.rs`: heal, inpaint and its preview, `with_layer_plane`, `painting_layer_plane`, dodge/burn, bucket fill, Clear, the retouch-output deposit, red-eye); `LayerRaster::unseeded` for `rendered_group`'s synthetic (its wasted view seed dropped); `grow_region` over a sample closure; the carried nits (the `sample_at` test's redundant tail, the 13B-2c-1 contract's report-walk wording); `phase13b_tile_sharing` `rect_writers`. | Every existing test green, pixel output / `.rz` / PSD bytes unchanged, a rect writer on a 24 MP layer leaves 0 views (the fill: the one sampling-plate view), residual 28 call lines. |
| **13B-2c-2b** | Job control on `to_contiguous` (a `to_contiguous_with_job(JobContext)` for `doc_color_policy.rs:508`, the one whole-layer copy with a `JobContext`); tile-aware byte equality (`same_bytes` / `equals_image`) at `doc_rotate.rs:127`, `warp_native.rs:437`, `artwork_document.rs:86`, `doc_content.rs:28`, `doc_lock.rs:294–316`. (`doc_geometry_set.rs:161` is a resample READ, not a comparison — reassigned to 13B-2c-3's residual list.) | Every existing test green, `.rz`/PSD byte-identical, residual count recorded. |
| **13B-2c-2c** | Asset-side tiles: the `SourcePixels::Rgba8` / `live_filter_evaluate::source8` / `retained_source_render::color8` producers tiled, so `LayerRaster::shared` at `retained_source_render.rs:228` / `doc_retained_source.rs:251` tiles without doubling a placed source's bytes. | Every existing test green, `.rz`/PSD byte-identical, a placed source's bytes counted once. **Implemented 2026-09-25** (a dabbed 24 MP placed source: `layer_pixels` 97048576 / 0 views, was 192000000 / 96000000; residual 25 call lines). |
| **13B-2c-3a** | The persistence / export / preservation / thumbnail / canvas-image / plate-copy / promotion reads off `contiguous_image()` (11 of the 25 residual call lines): `LayerRaster::runs`; the `.rz` layer PNG and asset PNG writers on a transient `to_contiguous()`; the WARP original writer through `runs()` (`put_raw_raster`); the PSD planes through `component_planes(runs)`; the two flat-save loss scans over `runs().flatten()`; the thumbnail on a transient `to_contiguous()`; `layer_canvas_image` and `copy_layer` through `canvas_clip` + `row_segments`; `precision_conversion_structure` through `promote_raster`; the `.rz` layer reader and the WARP original reader `unseeded`; the reopen dedup (`same_bytes` → `reidentified` at `materialize_byte`); the carried nits (`core/CLAUDE.md`'s `unseeded` callers, `contact_sheet.rs`'s `color8_image`); `phase13b_tile_sharing` `placed_source/save` views + `placed_source/reopen`. | Every existing test green, `.rz`/PSD byte-identical, a save / PSD export / thumbnail / reopen leaves 0 views, a reopened placed layer shares its asset's tiles, residual 14 call lines. **Implemented 2026-09-25** (`placed_source/save` 0 views, was 192000000 / 2; `placed_source/reopen` `layer_pixels` 96000000 / 0 views, was 192000000 / 192000000; `rect_writers/content_aware_fill` 0 / 0, was 96000000 / 1). |
| **13B-2c-3b** | The resample-input reads (`doc_transform.rs:597/601`, `doc_geometry_set.rs:161`, `retained_source_render.rs:311`'s Sample arm, `live_filter_evaluate.rs:346`'s filtered read) and the temp-document reads (`doc_content_aware.rs:163`, `doc_retouch_output.rs:317`, whose `deposit_into` takes `&RgbaImage`); the disposition of the seven unlisted residual sites (`doc_perspective.rs:329`, `doc_warp.rs:76`, `doc.rs:1512`, `doc.rs:1709`, `ffi_retouch.rs:195`, `live_filter_profile.rs:122`, `precision_scoped_source.rs:208`). | Every existing test green, pixel output / `.rz` / PSD bytes unchanged, residual count recorded. **Implemented 2026-09-25** (residual 0; the 24 MP cold transform rows leave 0 views, were 120000000–227604512 bytes / 2 views; 13B-2 closed). |
| **13B-3a** | The typed ACCESSOR (13B-2a's twin): `PrecisionImage::contiguous_rgba16/32` (bind-once, by value), `to_rgba16/32`, `ptr_eq`, `same_samples`; `as_rgba16/32` removed; every `core/src` reader, the tests and the example migrated (identity oracles → `ptr_eq`, raw-address oracles kept on the view); the black-box `precision_image_accessor` suite; carried nits 1–2. Representation still `Arc<Vec<_>>`. | Every existing test green; every pixel output, `.rz`/PSD byte, FFI signature and report key unchanged; `as_rgba16/32` gone; residual recorded. **Implemented 2026-09-25** (residual 76 call lines in 49 files). |
| **13B-3b-1** | The FLIP inside `precision_image.rs`: private `Plane<S>` = `Tiled(TilePlane<S>)` / `Contiguous(Arc<Vec<S>>)` under `Pixels::U16/F32`, a private identity token, `tile()` in every constructor (seeding the cache; planes above `CONTIGUOUS_CACHE_BYTES` stay contiguous), every accessor over both arms; 13B-2b's view cache moved to `contiguous_views.rs` and generalized over three view types; `pinned_rgba16/32` behind the two FFI pointer exports; the tiling-transient term in `retained_source_render::work_plan`; `precision_color`'s dedup on `plane_key()`; the six raw-address test oracles onto `ptr_eq`; `phase13b_tile_sharing` `typed/*`. | Every existing test green; typed projection and native save byte-identical to HEAD (`sha256`); the transient term planned; residual recorded. **Implemented 2026-09-25** (residual 72 call lines; `typed/dab` 1728000000 bytes / 9 planes, views after the projection 384000000 ≤ 1 GiB). |
| **13B-3b-2** | `rebuilt` / `with_rect` / `unseeded` / `reidentified` on `PrecisionImage` and their first callers (`render_high_depth`'s Shared arm, the whole-image typed writers); the typed constructors' seeds dropped where nothing reads the plane whole (e.g. the 16-bit structure promotion); `precision_image.rs` split into two `#[path]` children. `with_rect`/`read_rect` land with their suite; production rect callers are 3d. | Every existing test green; typed output byte-identical; sharing measured. **Implemented 2026-09-25** (`typed/dab` `precision_pixels` 1728000000 → 208777216, `typed/promote` views 192000000 → 0; `precision_image.rs` 772 → 439 lines). |
| **13B-3c-1** | Typed READS off the views, part 1 (13B-3c SPLIT at its spec): `PrecisionImage::row_segments` / `sample_at` / `runs_rgba16` / `runs_rgba32` (`PrecisionRun`, `PrecisionRowSegments`; `plane::PlaneRows`, `Plane::row_segments` / `sample_at`, the ONE u16 read rule `normalized16`); the typed compositor's raster arm, `composite_pixels_into`, `typed_shape` and the knockout unit pass through `row_segments` (`StyledPixels::visit_row`), the regional typed tree through `sample_at` (`arm_sample`, `direct_sample16`), `precision_geometry::image_equal` over `runs_rgba16/32`; `precision_image_reads` / `precision_compositor_parity`; `phase13b_tile_sharing` `typed/regional_window` / `typed/float_projection`. The row's "content-box" has no typed site (`scan_content_box` is byte-only, `doc_group_query.rs`) and its "persistence reads" are 13B-3e's (that row: `.rz` / PSD / export). | Every existing test green; typed projection / window / parity `sha256` equal to HEAD at u16 and f32; a typed projection / regional window leaves 0 views over the document; residual recorded. **Implemented 2026-09-25** (residual 72 → 69 call lines; `typed/projection` / `typed/regional_window` / `typed/float_projection` 0 view bytes over the document). |
| **13B-3c-2a** | The consumers of a typed projection handle onto the tile reads (13B-3c-2 SPLIT at its spec: consumers first): `PrecisionImage::rgba16_at` (the STORED u16 code, no view); `quantize_with_context` over `row_segments` rows and `quantize_scratch_bytes` from the dimensions (the u16 display / preview path and every quantizer caller); `tone_mapped_u16` over `runs_rgba32` (the float display, the float→u16 conversion); `precision_stats` / `float_stats` sample through `rgba16_at` / `sample_at` and histogram over `runs_rgba16/32`; `soft_proof::proof_render16_with` over `runs_rgba16`; `rz_precision_image_compare` over both images' runs; `precision_scoped_source`'s typed closure through `rgba16_at` (`mapped_index` deleted); carried nits 1–2; `precision_projection_consumers`; `phase13b_tile_sharing` `typed/consumers`. | Every existing test green; every consumer's answer, checkpoint count and report log byte-identical to HEAD; each consumer leaves 0 views after an eviction; warm `preview_rgba8` / `display_rgba8` ≤ 1.10 × HEAD; residual recorded. **Implemented 2026-09-25** (residual 69 → 59 call lines in 38 files; every `typed/consumers` line 0 views after the cold call, was 96000000–384000000; warm preview 0.75 ×, display 1.04 × HEAD). |
| **13B-3c-2b** | The projection OUTPUT unseeded: the render / regional / style-bake constructors (`precision_render.rs:766, 775`, `precision_regional.rs:230, 239`, `precision_style_render.rs:919`), after classifying the other projection readers 13B-3c-2a's survey found — `ffi_retouch.rs:43` (the retouch-plate tap walk), `precision_clipboard.rs:231/233` (Copy Merged), `precision_conversion_narrow.rs:758/768`, `precision_conversion_structure.rs:747`, `precision_color_policy.rs:347` (`doc_export.rs:1317`, `doc_export_layers.rs:315` and `precision_codec.rs:742` stay 13B-3e's). | Every existing test green; typed output byte-identical; a projection seeds no view (`precision_image_reads` (e) re-pinned at 0); residual recorded. **Implemented 2026-09-25** (`OutputView`; residual 59 → 53 call lines in 36 files; `typed/projection` / `typed/float_projection` / `typed/regional_window` 0 view bytes, were 384000000 / 192000000 / 16777216; every `sha256` and `unseeded` record line equal to HEAD; bounded ms medians ≤ 1.04 × HEAD. The style bake `precision_style_render.rs:919` stays seeded (D1, → 13B-3e); `ffi_retouch.rs:43` is a reader of a Seeded plate (→ 13B-3c-2c)). |
| **13B-3c-2c-1** | 13B-3c-2c SPLIT at its spec, part 1: `PrecisionImage::transient_rgba16/32` (a contiguous plane borrowed, a tiled one a transient owned copy, no view); `precision_sampling.rs:65/90/183`'s resample-input binds onto it (13B-2c-3b's rule); the retouch PLATES — the Current arm (`precision_retouch_source.rs:64`) copies the layer's rows into an unseeded plate, the cut render (:101) `Unseeded` — and their readers (`precision_retouch.rs:388` backdrop by rows, `:427` and `ffi_retouch.rs:43` transient, `precision_inpaint.rs:189` window rows; `ffi_retouch.rs:46` unseeded); the selection source (`precision_selection.rs:125` `Unseeded`, the canvas copied by rows in `canvas_samples`); `tone_mapped_u16`'s OUTPUT classified per caller (`view`); the classification of these constructor sites (decisions); carried nit (the fractional-alpha narrow preview line); `precision_plates`; `phase13b_tile_sharing` `typed/plates`. | Every existing test green; typed output byte-identical; plates, selection sources and resample inputs leave no view; every classified site's reader named; residual recorded. **Implemented 2026-09-25** (residual 53 → 45 call lines in 31 files; `typed/plates` `retouch_source_*` / `transform_retouch_source` `views_held` 0, were 240000000 / 192000000 / 384000000, `select_luminance_layer` `views_after` 0, was 48000000; every `plates` / `unseeded` record line and `typed/plates` `sha256` equal to HEAD; bounded warm medians ≤ 1.042 × HEAD). |
| **13B-3c-2c-2** | The other 19 of 13B-3b-2's 29 constructor sites classified seeded / unseeded with their readers named: `artwork_render.rs:142`, `live_filter_evaluate.rs:396/423`, `raw_local_develop.rs:264`, `retained_source_render.rs:271`, `doc_export.rs:1117/1329`, `doc_export_layers.rs:135`, `precision_warp.rs:117`, `raw_source_geometry.rs:466`, `precision_paint.rs:300`, `precision_insert.rs:132/141`, `precision_clipboard.rs:226/262`, `precision_clipboard_convert.rs:117`, `precision_scalar_filter.rs:53/156`, `convert`'s `from_float_samples` (`precision_image.rs:508`); 13B-3c-2c's share of the above-`CONTIGUOUS_CACHE_BYTES` item. Also: repair the stale 9g probe reader (`scripts/live-filter-probe/native.py`). | Every existing test green; typed output byte-identical; every constructor site classified with its reader named; residual recorded. **Implemented 2026-09-25** (all 19 classified — 17 Seeded with their whole readers and flip owners, the Copy / Cut payload `precision_clipboard.rs:226/262` and `convert` `precision_image.rs:538` Unseeded; `encode` and `converted` walk runs; residual 45 → 41 call lines in 29 files; `typed/clipboard` `copy_layer` / `copy_merged` / `copy_layer_float` `views_held` 0, were 192000000 / 192000000 / 96000000; every `seeds` line and `typed/clipboard` `sha256` equal to HEAD; the 9g reader peels 45/46 + `LIDS`). |
| **13B-3d-1** | 13B-3d SPLIT at its spec, part 1 — the typed PAINT rect writers: 7D `paint` / `erase` / `clone` / `heal` (`precision_paint.rs`, `precision_heal.rs`), the five native tools (`precision_paint_tools.rs`: `paint_blend`, `erase_native`, `dodge_burn`, `bucket_fill`, `gradient`) and the float Cut's erase (`precision_clipboard.rs` `erased_float`) through `read_rect_rgba16/32` + `with_rect_rgba16/32` over `layer_rect.rs`'s new PLANE forms (`plane_clip`, `canvas_rect_on_plane`, `coverage_rect16`, `plane_rect_offset`; the byte forms delegate); `paint_plan`'s storage gate and the clone/heal `SourceView` without a view; bucket fill's grow over one transient; the contiguous-fallback rect test (carried nit 1); `precision_rect_writers`; `phase13b_tile_sharing` `typed/dab` keys + `typed/dab_blend`; carried nits (the `copy_layer` cause, `native.py`'s identity word). | A typed dab retains ≤ 4 tiles WITHOUT a whole-plane walk and leaves no view (measured); every answer, refusal and plan JSON byte-identical to HEAD at 16-bit and float; typed dab `ms_median` ≤ 0.50 × HEAD; residual recorded. **Implemented 2026-09-25** (24 MP `typed/dab` / `typed/dab_blend`: `precision_pixels.unique_bytes` 208777216, views 192000000 = the base's own seed, were 960000000 / 5 views; `ms_median` 107.67 → 10.89 ms (0.10 ×) / 238.98 → 11.19 ms (0.05 ×); checkpoints per dab 25585 → 627 / 18232 → 29; all 181 `rect` lines equal to HEAD; residual 41 → 36 call lines in 26 files). |
| **13B-3d-2** | 13B-3d part 2 — the remaining typed rect / whole-image writers' binds: `precision_retouch`'s in-place arm (the app's 16-bit Clone Stamp / Healing Brush / Spot Heal / Content-Aware Fill / red-eye), `precision_scalar_edit`'s RGBA arm, `precision_lock::freezing_alpha`'s restore (`src` / `dst`), the whole-image writers' binds `selected_adjustment` / `precision_mask_apply` / `precision_color_policy` / `precision_adjustment`, and the `from_float_samples` rebuild twin (`precision_adjustment.rs:296`, `float_document.rs:620`). | These writers bind no whole view (a rect where one bounds the edit, rows / runs + `rebuilt` otherwise); output byte-identical at 16-bit and float; a retouch dab retains ≤ 4 tiles; residual recorded. **Implemented 2026-09-26** (`precision_retouch_apply::RetouchKernel` + one rect in place; the lock restore over `unshared_rect`; runs + `rebuilt_*` / `rebuilt_float_samples` elsewhere; `float_document.rs:620` needs no twin. 24 MP `typed/retouch_dab` / `typed/locked_dab`: `precision_pixels.unique_bytes` 208777216, views 192000000 = the base's seed, were 960000000 / 768000000; `ms_median` 776.20 → 599.83 ms (0.77 ×) / 80.57 → 11.12 ms (0.14 ×); after the review fix pass (tile-band walks) `typed/scalar_dab` 0.85 × MET and `typed/whole_writers` `apply_adjustment` warm 1.02 × HEAD MISSED its ≤ 1.00 × bound (first cut 1.04 × / 1.06 ×); all 147 `retouch` lines equal to HEAD; residual 36 → 24 call lines in 19 files). |
| **13B-3d-3a** | 13B-3d-3 SPLIT at its spec, part 1 — the Seeded typed LAYER producers that flip through an existing unseeded constructor or the `rebuilt_*` tiling policy: `rebuild_over` stops seeding (all eight `rebuilt_*` callers, incl. the no-change adjustment and the native tools' / retouch GROWN extent), `precision_merge.rs:301/361/390` and `render_seeded_with_context` (the members render), the bake (`baked_projection`), `artwork_render.rs:142`, `precision_warp.rs:117`, `precision_paint.rs:305` (Rasterize), `precision_clipboard_convert.rs:123`, `precision_retouch.rs:446` (NewLayer), `float_to_u16` and the u16 narrowing's `leaf_u16`; `precision_layer_producers`; `phase13b_tile_sharing` `typed/producers` / `typed/producer_consumers`; carried nits 1 (reworded) and 2 (declined). | Each producer Unseeded with its readers named; output byte-identical; residual recorded. **Implemented 2026-09-26** (24 MP `typed/producers` `views_held` 0, were 192000000 / 384000000 (bake); warp 48000000 = its source, was 96064016; warm medians 0.94–1.03 × HEAD; all 586 `producer` / `retouch` / `rect` / `unseeded` / `seeds` / `plates` record lines equal to HEAD; seeded sites 47 → 35 lines; residual 24 call lines, unchanged). |
| **13B-3d-3b** | 13B-3d-3 part 2 — `PrecisionImage` / `LayerRaster` FILLED constructors over `TilePlane::filled` for `precision_insert.rs:132/141` (`add_layer_at`), the sourced shapes (`precision_conversion_structure.rs:389`) and the narrowing's masked-adjustment plane (`precision_conversion_narrow.rs:270`); the resample outputs (`precision_sampling.rs:88/109/203`) under a per-caller `OutputView` — `precision_geometry.rs:151/154`, `retained_source_render.rs:373`, `develop_inspect.rs:89` Unseeded, `doc_export.rs:1316` Seeded. Pulled forward from 13B-3e: `precision_codec::encode` (the typed native save, `precision_codec::save`, the 16-bit export) walks `runs_rgba16`. | Each Unseeded site with its readers named; a filled plane shares one tile per shape; output byte-identical; residual recorded. **Implemented 2026-09-26** (`fill` child, `LayerRaster::filled`, per-caller `OutputView` on `transform_image` / `resize_image`, `encode` over runs; residual 24 → 23 call lines; 24 MP `add_layer` plane 192003072 → 1227776 bytes and 0 views; the geometry lines 0 views; `save_after_flatten` 0 views; every record line equal to HEAD `e250ba6`). |
| **13B-3e-1** | 13B-3e part 1 (13B-3e SPLIT into four runs at its spec): the typed export chain, the 16-bit PSD writer and the float writers off the views — the reads `doc_export.rs:1097/1329` (`convert16` walks `runs_rgba16`; the sharpen feeds the new `ops_photographic::unsharp_mask_runs`), `doc_export_layers.rs:128/316` (the crop reads ONE `read_rect_rgba16`; the trim scans alpha in TILE-BAND order through the private `Extent`), `float_io.rs:181` and `float_native.rs:73` (`runs_rgba32`), `psd_export16.rs:363/499` (the private `samples16` over runs; `composite_planes16` takes the image); and the Seeded sites `doc_export.rs:1117/1271/1301/1320/1340`, `doc_export_layers.rs:135/312`, `precision_io.rs:123` and the style bake `precision_style_render.rs:919`, all Unseeded (`OutputView::Seeded` keeps no caller, `#[allow(dead_code)]`). | Residual 23 → 15 call lines; seeded sites 33 → 24; every output byte, refusal, checkpoint and report identical to HEAD `2c04c47`; `typed/exports` 0 views on every line; warm ≤ 1.05 × HEAD. **Implemented 2026-09-26** (residual 23 → 15, `rg -c` 25 → 17 in 13 files; seeded sites 33 → 24; all 72 `export`, 92 `filled` and 26 `unseeded` record lines identical to HEAD; every `typed/exports` line 0 views; warm 0.89–1.05 × HEAD on eight lines, `save_psd_flattened` 1.08 × and `save_native_float` 1.28 × MISSED — see the verification record). |
| **13B-3e-2** | The live filter and the retained source off the views: the reads `live_filter_evaluate.rs:409`, `live_filter_preview.rs:160`, `retained_source_render.rs:252`; the Seeded sites `live_filter_evaluate.rs:396/423`, `retained_source_render.rs:272`. | Residual 15 → 12; output byte-identical; the live-filter warm lines measured against HEAD. **Implemented 2026-09-26** (`straight_plane` / `filtered` over runs, `color16` over a row buffer, `from_rgba16_unseeded` at `live_filter_evaluate.rs:396` and `retained_source_render.rs:272`, `render8`'s 16-bit arm `LayerRaster::unseeded`; the preview's tile row cursor MISSED (1.23 × → 1.20 × HEAD after the fix pass), so `live_filter_preview.rs:160` and its feeding seed `live_filter_evaluate.rs:423` REVERTED to HEAD's: residual 15 → 13 call lines (`rg -c` 17 → 15 in 11 files), seeded sites 24 → 22; all 44 `live` record lines identical to HEAD `3f7e2e5`; `typed/live_filter` warm 0.95–1.005 × HEAD after the revert, `views_held` 48000000 on the typed edit lines (the seeded filtered plane, were 96000000), 0 on the byte-canvas edit and the unfiltered preview). |
| **13B-3e-3a** | 13B-3e-3 SPLIT at its spec, part 1 — the installed-layer / caller-image whole readers and the scalar filter off the views: `selected_filter.rs:263` (Pixelate rows through `row_segments`), `selected_auto_levels.rs:52` (TILE-BAND histogram, row-major `auto_color` runs), `precision_plane_read.rs:199` (TILE-BAND), `precision_cutout.rs:74` (TILE-BAND scan), `warp_native.rs:217` (row-major runs), `precision_warp.rs:85` (ONE transient); the Seeded `precision_scalar_filter.rs:53/156`; the layer a direct in-memory payload handle installs (no code). | Residual 13 → 7 call lines; seeded 22 → 20; record lines identical to `a2541cf`; `typed/whole_readers` 0 views; warm ≤ 1.05 ×. **Implemented 2026-09-26** (residual 13 → 7, `rg -c` 15 → 9 in 5 files; seeded sites 22 → 20; all 60 `whole` and 54 `producer` record lines identical to HEAD `a2541cf`; every `typed/whole_readers` line `views_held` 0; warm 0.53–1.01 × HEAD after the fix pass (first cut: `selected_auto_color` 1.09 × and `refinement_cutout` 1.61 × MISSED); the warp keeps its transient, 1.01 ×). |
| **13B-3e-3b** | 13B-3e-3 part 2 — raw develop / geometry / inspection off the views: `raw_local_develop.rs:163/183`, `develop_inspect.rs:136` (the develop preview is INTERACTIVE: warm bound fixed before measuring, HEAD's view kept on a MISS) and the Seeded `raw_source_geometry.rs:466`; `raw_source_geometry.rs:392` and the Seeded `raw_local_develop.rs:264` deferred to 13B-3e-4 (D1). | Residual 7 → 4 (`raw_source_geometry.rs:392` / `raw_local_develop.rs:264` deferred; 7 → 5 on a preview revert); output byte-identical. **Implemented 2026-09-26** (the develop's variant / base reads on `runs_rgba16`, passes 1 and 3 hoisting `job.every` per run; the preview MISSED after its fix pass, so `develop_inspect.rs:136` and `raw_source_geometry.rs:466` REVERTED to HEAD's: residual 7 → 5 call lines, `rg -c` 9 → 7 in 4 files; seeded sites 20 → 20; all 76 `raw` record lines identical to HEAD `58f8304`; `typed/raw_develop` warm 0.83–1.02 × HEAD after the revert; the 9f probe repaired and PASSED). |
| **13B-3e-4a** | 13B-3e-4 SPLIT at its spec into 4a / 4b / 4c, part 1 — the host intakes `ffi_precision_input.rs:76` / `host_pixels.rs:236` and the develop output `raw_local_develop.rs:302` (the status doc's earlier `:264`) Unseeded; `raw_source_geometry.rs:392` onto ONE transient that replaces their seed (13B-3e-3b D1); the FFI lend `precision_image.rs:307/323` onto a private materialization (`Plane::lent`), and the storage identity export `rz_precision_image_storage_identity` (the 9h Move oracle, D6). | `rg -c 'contiguous_rgba(16\|32)\('` 7 → 4 (2 definitions + `live_filter_preview.rs:164`, `develop_inspect.rs:136`); seeded sites 20 → 17; every record line identical to `6b97c48`; every bounded peak line tree ≤ HEAD. |
| **13B-3e-4b** | 13B-3e-4 part 2 — the reader-side `unseeded` decoders (`precision_png.rs:153`, `precision_codec.rs:495`, `psd_import16.rs:281/427`, `float_io.rs:158`, `float_native.rs:142`, `warp_native.rs:534`, `precision_clipboard_codec.rs:313/545/568`), each checked against its whole readers; the reopened empty layer filled; the narrowing's seeds (`precision_conversion_narrow.rs:233/400/127`); `OutputView::Seeded` and its arms removed (`precision_render.rs:789/805`, `float_document.rs:627`, `precision_sampling.rs:64/78`). | Seeded sites 17 → 2 (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:470`); native typed files byte-identical. **Implemented 2026-09-26** (`from_rgba16_decoded` / `from_rgba32_extended_decoded` over the private `Tiling::Decoded`, `uniform_tiles` filling a bitwise-uniform decoded plane; the narrowing's three planes `LayerRaster::unseeded`; `OutputView::Seeded`, `from_float_samples` and `from_rendered_float` removed; seeded sites 17 → 2, `rg -c` 4 unchanged; all 72 `decode` record lines identical to HEAD `277c88a`; every `peak_decoders` line tree ≤ HEAD (600×520: `held` 0–13523448 bytes lower, 0 only on `pipeline/png16_preview`, whose first preview materializes the view HEAD adopted); `typed/decoders` warm 0.95–1.04 × HEAD, `views_held` 0 on nine lines (were 96000000–192000000) and `w·h·8` on the two preview lines (the master preview was 576000000); no MISS, nothing reverted). |
| **13B-3e-4c-1** | 13B-3e-4c SPLIT at its spec, part 1 — the two kept preview reads (`live_filter_preview.rs:164` + its seed `live_filter_evaluate.rs:429`; `develop_inspect.rs:137` + its seed `raw_source_geometry.rs:471`) onto ONE narrow crate-private view accessor `preview_view_rgba16`, the kept exception (owner 13A-latency; no new tile-read attempt, D2); `contiguous_rgba16/32` and `View::into_rgba32` removed (`plane_key` / the storage identity stay: they read no view); every test / example reader migrated onto owned copies, per-sample reads and the storage identity; the new `preview_view_exception` suite; carried nits 1–4. | `rg -c 'contiguous_rgba(16\|32)\('` 0 over `core/src`, `core/tests`, `core/examples`; `rg -n 'preview_view_rgba16\('` 3 lines; every answer, record line and `views_held` equal to HEAD; typed suites green. **Implemented 2026-09-26** (421 calls / 406 lines migrated, 4 fn-path uses; `preview_view_exception` passes at HEAD `9a30000` and the tree; all `record_live_filter_reads` / `record_raw_develop_reads` / `record_host_intake` / `record_decoders` and `views` lines identical, every `peak_*` line equal; `phase7_precision --accuracy-only` identical; `typed/live_filter` / `typed/raw_develop` `sha256` and `views_held` equal). |
| **13B-3e-4c-2a** | 13B-3e-4c-2 SPLIT at its spec, part 1 — `OutputView` and its `view` parameters removed (9 functions: `PrecisionScene::render_with_context`, `PrecisionDocument::render_with_context` / `render_cut_with_context`, `tone_mapped_u16`, `analysis_source`, `transform_image`, `resize_image` and the private `output16` / `output32`, deleted); `tile()`'s f32 seed, `View::Rgba32`, `PlaneKey::Rgba32`; `from_rgba32` / `from_rgba32_extended` unseeded and the `from_rgba32_extended_unseeded` alias deleted; the seeded lend's f32 half; carried nits 1, 2 and 5 (`precision_image_build.rs` ≤ 600). | `rg OutputView core` empty; no f32 view type; every answer, checkpoint, report log and `.rz` byte identical to HEAD; f32 `views` 0, peak / held ≤ HEAD. **Implemented 2026-09-26** (`OutputView` 71 lines / 25 files → 0; `precision_image_build.rs` 616 → 594 lines; seeded-site `rg` 2 lines (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:471`); all 634 record lines (`f32` 32 new) and the `phase7_precision` output identical to HEAD `31981c3`; `views` `ctor/from_rgba32` / `ctor/from_rgba32_extended` / `lend/seeded_f32` 4992000 → 0 (control `ctor/from_rgba16` 2496000 in both); `peak` `held` 9984808 → 4992544 (both constructors) and 14976848 → 9984584 (the lend), every other peak / views line equal). |
| **13B-3e-4c-2b** | 13B-3e-4c-2 part 2 — f32 planes above `CONTIGUOUS_CACHE_BYTES` tiled — `tiles_of`, `uniform_tiles`, `precision_image_fill.rs:56` — with the tiling transient and the `transient_rgba32` / `Plane::lent` / `with_rect` copies of such planes admitted or bounded by a tracked-allocator peak analysis vs HEAD; the three `#[ignore]`d above-bound tests re-declared. | f32 above-bound planes tiled or their residual stated with evidence; no uncharged full copy above HEAD; typed suites green; native typed files byte-identical. **Closed as a recorded residual 2026-09-26** (the 8200×8193 all-0.0 float TIFF open refused at HEAD `d1f7601` and with the tiling code alike: "needs 2151953720 bytes … working limit is 2147483648", 2·P + 2110520 for P = 1074921600; no admitted document path retains an above-bound f32 plane; the tiling code backed out; tiling question → 13B-8; nits 1–3 closed). |
| **13B-4** | Byte masks/channels (`Arc<GrayImage>`) and typed `PrecisionPlane` → `TilePlane<u8>` / `<u16>`; mask derived/aligned caches per tile. | Mask/channel suites green; byte-identical persistence; mask-paint dab retains ≤ 4 tiles. |
| **13B-5** | History: measured per-edit incremental bytes (history report `incremental_history_bytes`) before/after for brush, mask-paint, adjust and move on the 24 MP corpus; `NamedSnapshots` budget re-measured. Recovery / `.rzsnapshot` on-disk sharing stays 13C. | Measured before/after figures recorded with load; budget decision recorded. |
| **13B-6** | One process-wide byte-bounded resident-cache registry (style planes, mask caches, LUT, content-box, live-filter) with eviction order and a report section, shared across documents/jobs. | Every cache bounded in bytes; report section present; eviction order tested. |
| **13B-7** | The regional path for the canvas display / UI previews; MCP `render` exposing the plan (`global_analysis`, `required_input_region`); the packaged byte base's guided member. | Owned MCP drive; regional display equals full projection. |
| **13B-8** | Tighter regional plan/progress figures (split at its spec). | Each handed figure tightened or its residual stated with cause. |
| **13B-9** | Sampling plates. | Plates regional or their full-frame cost stated and bounded. |

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

## 13B-1 contract

`core/src/tile_store.rs` (`pub mod tile_store`):

- `TILE_EDGE = 256`. `Sample` is sealed over `u8`, `u16`, `[u8;4]`, `[u16;4]`,
  `[f32;4]` with `same_bits` (integers `==`; f32 per-component `to_bits`) and no
  `PartialEq` bound.
- `Tile<S>` is opaque: `width`, `height`, `samples()` (row-major, stride =
  width, no padding), `bytes()`.
- `TilePlane<S>` (`Clone`, `Send + Sync`): `filled`, `from_contiguous`,
  `to_contiguous`, `read_rect`, `with_rect`, `rebuilt_from`, `dimensions`,
  `columns`, `rows`, `tile`, `tile_rect`, `shared_tile_count`,
  `retained_bytes`; crate-internal `_with_job` variants and
  `record_resources(inventory, category, cache)`.
- `accounting(&[&TilePlane<S>]) -> TileAccounting { unique_tiles,
  referenced_tiles, unique_bytes, referenced_bytes, index_bytes }`, unique by
  tile sample pointer (as `Inventory::record`).
- Geometry: tile `(tx, ty)` covers `[tx·256, min(tx·256+256, w)) × [ty·256,
  min(ty·256+256, h))`, `columns = ⌈w/256⌉`, `rows = ⌈h/256⌉`, edge tiles
  clipped.
- Refusals by name: `tile plane dimensions exceed the supported pixel limit`
  (`w == 0 || h == 0 || w·h > MAX_PIXELS`); `tile plane sample count does not
  match dimensions`; `tile rect leaves the plane` (`read_rect` → `None`);
  `could not allocate tile storage`. Zero-area: `with_rect` `Ok(None)`,
  `read_rect` `Some(vec![])`.
- `filled`: at most 4 tile allocations (interior, right column, bottom row,
  corner shapes).
- Latch: `with_rect` / `rebuilt_from` keep the old `Arc` when every sample is
  `same_bits`, allocate only a changed tile, mint a fresh index, `Ok(None)` when
  nothing changed.
- Cancellation: one `job.check()` per visited tile (`with_rect` visits its
  intersecting tiles, the others every tile); the public wrappers are the
  `precision_mask.rs` shape (`begin`, variant, `report(1.0, 5)`), so an
  uncancelled run ends at `visited + 2` checkpoints, stage 5, progress 1.0.
  Declared cancel-to-return bound `T_f + c_tile`.
- No FFI, header, Swift, `.rz` or report change; no document holds tiles yet.

## 13B-2a contract

`core/src/layer_raster.rs` (`pub mod layer_raster`, re-exported as
`rasterize_core::doc::LayerRaster`):

- `LayerRaster` derives only `Clone` (`Send + Sync` by construction); no
  `Deref`, no `PartialEq`, no `Debug`. Private representation: in 2a the SAME
  `Arc<RgbaImage>` allocation `Layer.pixels` held.
- `new(RgbaImage)` (a fresh identity), `shared(Arc<RgbaImage>)` (adopts the
  `Arc`, no copy, identity kept), `dimensions` / `width` / `height`.
- `contiguous_image() -> Arc<RgbaImage>` — the legacy whole-image view and
  THE tracked residual; bound ONCE per raster per operation, at the top of the
  owning function or block, never inside a loop body or a per-row / per-sample
  closure.
- `to_contiguous() -> Result<RgbaImage, String>` — an owned copy admitted
  through `precision_image::reserved_vec`; refusal "could not allocate layer
  raster storage". Clone-then-mutate sites use it with `.ok()?` in `Option`
  funnels and `?` in `Result` ones.
- `read_rect(x, y, w, h) -> Option<Vec<[u8; 4]>>` — `TilePlane::read_rect`
  semantics: row-major samples, zero-area `Some(vec![])`, a rect leaving the
  plane `None`, bounds checked in u64 (`read_rect(u32::MAX, 0, 2, 1)` and
  `read_rect(0, u32::MAX, 1, 2)` are `None`, never a wrap and a slice panic).
- `ptr_eq` (allocation identity), `weak_count` (live key handles; test oracle
  only), crate-internal `key() -> LayerRasterKey` and
  `record_resources(inventory, category, cache)` — the SAME two pointers
  (`shared_buffer_structure` + the sample `Vec` under `category`) the
  inventory recorded before, so report bytes and keys are unchanged.
- `LayerRasterKey` (crate-internal, a `Weak`): `same` (`std::ptr::eq` on the
  weak pointers) and `is_dead` (`strong_count() == 0`). `style_cache::CacheKey`
  and `doc_group_query`'s content-box memo key on it, never on a
  `contiguous_image()`.
- Unchanged producers (`retained_source_render::color8`,
  `live_filter_evaluate::source8`, `doc_export::convert8`,
  `SourcePixels::Rgba8`, `FilteredPixels::Byte`, `rzdc_read`'s
  `pending_pixel`) stay `Arc<RgbaImage>` and are wrapped with
  `LayerRaster::shared` at the assignment.
- No FFI, header, Swift, `.rz`, PSD, report-key or `schema_version` change.

## 13B-2b contract

`core/src/layer_raster.rs` (every 2a signature kept):

- Private `Repr`: `Tiled(TilePlane<[u8; 4]>)` or `Contiguous(Arc<RgbaImage>)`
  (2a's code verbatim). Identity is a private `Arc<()>` token: `ptr_eq` is
  `Arc::ptr_eq`, `weak_count` `Arc::weak_count`, `key()` a `Weak<()>`
  (`LayerRasterKey::same` / `is_dead` unchanged). Clones share it; `new`,
  `shared`, `rebuilt`, `with_rect` (`Some`) and `reidentified` mint one.
- `new(image)` tiles and seeds the view cache with `Arc::new(image)`; when
  `TilePlane::from_contiguous` refuses (zero area, above `MAX_PIXELS`,
  allocation) it is `Contiguous`. `shared(Arc)` is always `Contiguous` — adopts,
  never copies, no seed — so a placed source's layer aliases its asset until
  its first write.
- `rebuilt(&self, image)`: tiled with equal dimensions → `rebuilt_from`
  (unchanged tiles shared; `Ok(None)` keeps the plane) with a FRESH identity
  and a seed; otherwise `new(image)`.
- `with_rect(x, y, w, h, samples) -> Result<Option<Self>, String>`: tiled →
  `TilePlane::with_rect` (`Some` = fresh identity, no seed); contiguous →
  compare the rect's bytes (`Ok(None)` when equal), else patch a
  `to_contiguous` copy into `new` (so the first write tiles it). Same refusals
  as the store (count mismatch, rect leaving the plane).
- `pub(crate) reidentified`, `byte_len` (`w·h·4`, no view),
  `shared_tile_count` (tiled/tiled only, else 0; test oracle).
- `read_rect` keeps 2a's semantics (bounds checked in u64 first, zero-area
  `Some(vec![])`); on tiles `None` past the bounds check is an allocation
  failure only — the carried nit is CLOSED: the paint callers intersect first
  and map `None` to the op's `None` (the old `.ok()?`), `covers` maps it to
  "not hit".
- The view cache: `CONTIGUOUS_CACHE_BYTES` = 1 GiB, `CONTIGUOUS_CACHE_ENTRIES`
  = 256, one `static Mutex`, entries `{ Weak` of the plane INDEX
  (`TilePlane::weak_index`), `Arc<RgbaImage>`, bytes, LRU tick `}`. At most one
  live entry per plane (insert removes any `Weak::ptr_eq` entry first); a miss
  materializes OUTSIDE the lock and re-checks before inserting; dead planes
  are pruned; a single view above the byte bound is returned but not
  retained. An allocation failure while materializing panics "could not
  allocate layer raster storage" (reported as `internal error: …` by the
  FFI's `catch_unwind`). `contiguous_view_bytes()` is the test oracle.
- `record_resources`: tiled → `TilePlane::record_resources` (index as
  `tile_structure`, tiles under the caller's category); contiguous → 2a's two
  records.
- `resources.rs`: the resource report and `evict_caches` (not
  `cache_inventory`, so conversion capacity plans and the live-filter
  eviction inventory never count disposable views) record the views of the
  supplied documents' planes as `contiguous_views` (cache, each view pinned);
  `evict_caches` drops every view and names "contiguous layer views" in
  `scope`; the report gains top-level `contiguous_view_bytes` (cache
  allocations in that category only — a view an authoritative owner also
  holds, e.g. a `SourcePixels::Rgba8` asset, is retained). `schema_version`
  stays 1.
- Writers: `with_layer_pixels` → `with_layer_raster(idx,
  pixels.rebuilt(image))`; `painting_layer` / `painting_layer_blend` walk only
  `doc_paint::overlay_rect` (the bounding box of overlay samples with any
  nonzero byte, intersected with the layer's canvas intersection) through
  `read_rect` + `with_rect` + `with_layer_raster`; a no-change Normal/Erase
  paint answers `reidentified` (Normal never refuses a no-change overlay).
  `doc_lock`, `doc_scope` ×2, `doc_content` ×2, `doc_color_policy`,
  `live_filter_profile` and the mask-apply bake in `doc.rs` go through
  `rebuilt`.
- Host: `RasterResources.trimCachesIfNeeded` compares `cache_bytes −
  contiguous_view_bytes` against the 128 MiB `cacheLimit`.
- No FFI, header, `.rz`, PSD or `schema_version` change.

## 13B-2c-1 contract

`core/src/tile_store.rs` / `core/src/layer_raster.rs` (every earlier
signature kept):

- `TilePlane::row_segments(y, x0, x1) -> RowSegments<'_, S>`: row `y`,
  columns `x0..x1` (exclusive), as the contiguous runs the tiles hold —
  `(x, samples)` per tile column, left to right, `x` the run's first column,
  runs adjoining (`x_next == x + len`). Empty when `y >= height` or
  `x0 >= x1`; `x1` is clamped to `width`. Sliced exactly as `read_rect`
  slices a row; no allocation.
- `TilePlane::sample_at(x, y) -> Option<S>`: `None` outside the plane; no
  allocation.
- `LayerRaster::row_segments` (`LayerRowSegments<'_>`) and
  `LayerRaster::sample_at`: both arms obey `TilePlane`'s rules verbatim.
  Tiled: the store's runs; contiguous: ONE run `(x0, &samples[y·w + x0 ..
  y·w + min(x1, w)])`. Neither touches the view cache.
- Readers: `doc::composite_layer_into`, `style_composite::composite_pixels_into`
  and the knockout unit pass (`style_knockout.rs`) read rows through
  `row_segments`; the layer-space span is computed ONCE per layer, `(0, 0)`
  unless the canvas span is non-empty (a layer right of the accumulator has
  `x1 < x0`, so no negative reaches the `u32` cast and the per-row
  `job.check()` sequence is unchanged). `mi` is always the PLANE index
  `ly·lw + lx` (mask reads); `li = i·4` is RUN-relative and only indexes
  `run.as_flattened()`. `Shape::of_layer` reads `px[3]` and indexes the mask by
  `y·lw + x`; the cutout's per-chunk opaque precondition reads
  `row_segments(y, x, x + width)`. The per-pixel bodies
  (`composite_layer_sample`, `composite_pixels_sample`,
  `Shape::coverage_byte`, the knockout statements) keep their signatures and
  order, so every output is byte-identical.
- The regional tree: `ResolvedNode.pixels` is REMOVED; `ResolvedNode::pixel`
  keeps its bounds check and answers `layer().pixels.sample_at(lx, ly)` (a
  raster's own pixel or a materialized group's synthetic).
  `doc_group_query::scan_content_box` reads `sample_at`.
- `resources.rs`: `layer_raster::contiguous_views_of(&walked)` clones the
  views of every plane a walked `Inventory` recorded (under the lock) and
  `record_contiguous_views(views, inventory)` pins and records them; the
  report matches views against its whole walk (documents, images, typed
  documents, typed images, raw local masks, and `cache_inventory`),
  `evict_caches` walks the documents
  ONCE for both `before` and `after`, and its `scope` names the process-wide
  view drop ("… and every contiguous layer view (removed_cache_bytes counts
  only the supplied documents' views)").
- A projection (`flattened`) or a regional window leaves 0 views behind.
  `unique_retained_bytes` can move between two consecutive reports when
  another thread evicts the views in between; the report is a point sample.
- No FFI, header, Swift, `.rz`, PSD, report-key or `schema_version` change.

## 13B-2c-2a contract

`core/src/layer_rect.rs` (new, private; every rect layer-space `(x, y, w,
h)` with `w, h > 0`, `None` = empty; the rect arithmetic allocation-free):

- `canvas_clip(canvas, layer)`: the layer's canvas intersection, the clip
  `doc_paint::overlay_rect` makes (named there as the master copy; i64).
- `canvas_rect_on_layer(canvas, layer, (x0, y0, x1, y1))`: a canvas rect
  (exclusive ends) ∩ `canvas_clip`.
- `coverage_rect(mask, canvas, layer)`: the bbox of canvas-frame u8 samples
  above 0 inside `canvas_clip`.
- `nonzero_rect((w, h), set)`: the bbox of the row-major samples `set(i)`
  holds for.
- `rect_image(pixels, rect)`: `read_rect` flattened into an `RgbaImage` with
  no second copy (`None` = allocation failure).
- `rect_offset(layer, rect)`: `offset + (x, y)`, the canvas position a rect
  image stands at.

`LayerRaster::unseeded(image)` is exactly `new` without the view-cache seed
(the same contiguous fallback); `rendered_group`'s synthetic is built with it
(it is read only through `row_segments` / `sample_at`, so no output moves).
`layer_raster::ALLOCATION` is `pub(crate)` so the rect writers' allocation
failure raises the same sentence `to_contiguous` did.

The rect walk, one shape at every site: rect → `read_rect` → the op's
existing per-pixel body over the rect buffer with rect-relative indices →
`with_rect` → `with_layer_raster`. Per-pixel statements keep their order and
arithmetic, so output is byte-identical. `with_rect` shares every tile the
walk left bitwise unchanged, including tiles inside the rect — the rect
bounds the READ, the diff bounds the copies.

| Site | rect | helper `None` → | `with_rect` `Ok(None)` → |
| --- | --- | --- | --- |
| `with_layer_plane_unlocked` | `canvas_clip` | `None` | `None` |
| `painting_layer_plane_unlocked` | `overlay_rect` | `None` | `None` |
| `dodge_burn_layer_unlocked` | `overlay_rect` (a superset of the alpha it reads) | `None` | `None` |
| `bucket_fill_unlocked` | `nonzero_rect` of the grown region (never empty) | — | `reidentified` |
| `clear_selection_unlocked` | `coverage_rect(mask)` | `reidentified` | `reidentified` |
| `deposit_into` | `nonzero_rect` of the domain | `None` | `None` |
| `red_eye_layer_unlocked` | `canvas_rect_on_layer` of its clipped rect | `None` | `None` |
| `heal_layer_unlocked` | `canvas_rect_on_layer(∪ box_padded)` | `Ok(None)` | `Ok(None)` |
| `inpaint` | `canvas_rect_on_layer(∪ plans[k].window, k in order)` | `Ok(None)` | `Ok(None)` |

- `with_rect` `Err` maps as the old `to_contiguous` error did (`.ok()?` /
  `None` in the Option funnels, `?` in the Result funnels); `reidentified`
  reproduces the old `with_layer_pixels`-on-unchanged-bytes answer,
  `freezing_alpha`'s latch to `None` on a transparency-locked layer
  included.
- `grow_region(dimensions, pixel, seed, tolerance, contiguous)`: bucket fill
  grows over `sample_at` (the `doc_select.rs:127` view is gone), Magic Wand
  over its composite's bytes.
- Heal and inpaint hand every window helper (`heal_window_into_pixels[_with_context]`,
  `fill_component`, `dest_window`, `paste_window_into_pixels`) `rect_image`
  as the image and `rect_offset` as the offset: a window pixel is reachable
  through the rect exactly when it is on-canvas and on-layer, so
  `layer_byte` / `usable` / the solve contour are unchanged. One rect per
  call, threaded through every component (`doc_heal`'s "ONE clone" rule,
  now ≤ the layer's canvas intersection). Heal runs `over_window_limit` for
  every part, in `parts` order, BEFORE the rect (the in-loop check moved,
  not duplicated). Inpaint keeps `control.report(0.18, 3)` / `control.check()`
  around its read.
- The inpaint preview heals `reduced_dest` in place
  (`heal_window_into_pixels_with_context` on `(0, 0, rw, rh)`), with no temp
  document: `temp`, `preview_pixels`, `healed`, `healed_pixels` and the
  `doc_inpaint.rs` view are gone.
- Red-eye's coverage pass and write pass share ONE rect buffer through
  `rect_byte(rx, ry)` (`None` off the layer); `coverage` / `pupil_mask` /
  `feather_mask` keep the canvas-clipped grid. The `doc_redeye.rs` view is
  gone.
- No new checkpoint anywhere, so no cancellation bound moves.
- No FFI, header, Swift, `.rz`, PSD, report-key or `schema_version` change.
  `with_rect` / `read_rect` / `reidentified` / `with_layer_raster` signatures
  unchanged.

## 13B-2c-2b contract

`tile_store.rs` (`impl<S: Sample> TilePlane<S>`), the ONLY tile equality —
a pointer short-circuit per tile, then `Sample::same_bits` per sample; no
allocation, no job:

- `same_bits(&other)`: `false` on differing dimensions; else every tile
  index `Arc::ptr_eq` or bitwise equal sample for sample.
- `equals_contiguous(&[S])`: `false` unless the slice holds exactly `w·h`
  samples; else each tile's rows against the row-major plane at `(y + r)·w +
  x` (`rebuilt_from_with_job`'s slicing).

`job_control.rs`: `JobContext::cancelled(self) -> bool` reads the flag only —
no checkpoint, no park, no report; `NONE` answers `false`.

`layer_raster.rs` (every existing signature kept):

- `same_bytes(&other)`: `false` on differing dimensions; (Tiled, Tiled) →
  `TilePlane::same_bits`; (Contiguous, Contiguous) → `Arc::ptr_eq` or the
  `w·h` samples equal; in BOTH mixed arms the TILED raster's `equals_raw`
  receives the CONTIGUOUS side's image. Symmetric; never touches the view
  cache.
- `equals_image(&image)` = `equals_raw(image.dimensions(), image.as_raw())`;
  `equals_raw` (`pub(crate)`) is `false` unless the dimensions match and the
  buffer holds ≥ `w·h·4` bytes, and compares exactly the first `w·h·4`.
- `to_contiguous_with_job(job)` (`pub(crate)`): Tiled →
  `materialize_with_job` (one `job.check()` per tile inside
  `TilePlane::to_contiguous_with_job`); Contiguous → one `job.check()` then
  2a's copy. `to_contiguous` is it with `JobContext::NONE`.
  `materialize_with_job` maps the store's error to the control's own
  sentence when `job.cancelled()`, else to `ALLOCATION` — never by matching
  text; `view()` passes `NONE`, so the legacy path still answers
  `ALLOCATION`.

The five former `contiguous_image()` comparison sites:
`doc_rotate::exact_geometry_changed` (`same_bytes`),
`warp_native::attach_byte` (the raw original compared with `equals_raw`
BEFORE any raster is built; `LayerRaster::new` only when unequal),
`artwork_document` and `doc_content` (`equals_image`), and
`doc_lock::freezing_alpha` (the restore reads `before` through
`row_segments` — `at` indexes the plane, the run is zipped — the latch is
`equals_image`; `rebuilt(restored)` stays).

**Checkpoint accounting of `rz_doc_convert_to_profile_with`** (sRGB →
Display P3, default options, one 6000×4000 RGBA8 raster = 384 tiles):
`begin` = 1 (`job_control.rs:124`), the post-preflight `job.check()`
(`doc_color_policy.rs:474`) = 2, `job.reserve` (`doc_color_policy.rs:484` →
`job_control.rs:104`, a checkpoint) = 3, the tile walk = 4…387, the first
`color8` poll = 388. The contiguous twin: its single Contiguous-arm check =
4, its first `color8` poll = 5. An uncancelled tiled run therefore polls
exactly 383 more checkpoints than its twin; the per-4096-pixel `color8`
polls are unchanged. The `ALLOCATION` arm has no black-box seam (it is
verified by reading); the cancel arm adds NO checkpoint.

**Declared bound** (written before measuring): a cancel issued inside the
tile walk returns within `T_f + c_tile`, `T_f` = `RETURN_FLOOR_S` (1 ms),
`c_tile` = the fastest of three uncancelled `LayerRaster::to_contiguous()`
runs on the same 6000×4000 raster divided by 384 (`TILE_EDGE` 256: 24×16
tiles). A probe whose return lies outside checkpoints 4…387 is a VOID
measurement (the poller fell behind), not a MISS.

No FFI, header, Swift, `.rz`, PSD, report-key or `schema_version` change.

## 13B-2c-2c contract

Every byte source PRODUCER answers a `LayerRaster`; `LayerRaster::shared` is
no longer called by a render or a Place (its one production caller left is
the `.rz` reader's 0×0 placeholder). Core-only: no FFI, header, Swift, `.rz`,
PSD, report-key or `schema_version` change; every pixel output and `.rz` byte
stream unchanged.

- `source_asset.rs`: `SourcePixels::Rgba8 { pixels: LayerRaster, profile }`;
  `SourceAsset::rgba8() -> Option<&LayerRaster>`; `record_resources` records
  the pixels through `LayerRaster::record_resources(inventory,
  "embedded_source_pixels", false)` (the image struct's
  `embedded_source_structure` record is gone; the `SourceAsset` object record
  stays). `SourceInput::asset` (Byte) CLONES the input layer's raster — the
  asset keeps the input layer's identity (adopt, never copy; the tiled twin
  of the old `contiguous_image()` alias). A decoded asset
  (`native_sources_assets.rs`) is built `LayerRaster::unseeded`.
- `retained_source_render.rs`: `Colored8 { Raster(LayerRaster),
  Owned(RgbaImage) }` — `Raster` is a plane that already lives somewhere (the
  asset's, or the live-filter cache's); `Owned` is a transient converted plane,
  tiled only by the caller that stores it. `dimensions`, `sample_at` (`Owned`
  → `get_pixel_checked`) and `into_raster` (`Raster` → `reidentified()`,
  `Owned` → `unseeded`: always a fresh identity, never a seed). `color8(&LayerRaster,
  ..) -> Colored8` answers `Raster(source.clone())` for the identity (equal
  profile bytes or no `Transform`), else `Owned`, walking the raster's runs;
  `color8_image(&Arc<RgbaImage>, ..)` is the pre-13B-2c-2c `color8` for the
  contact sheet. Both run ONE private per-pixel loop, `convert8_runs`, whose
  `job.every(i)` takes the RUNNING sample index, so every poll lands on the
  sample it did before. `render8`'s byte arm: Shared → `colored.into_raster()`;
  Sample → one whole-plane bind (`Raster` → `contiguous_image()`, `Owned` → the
  image itself) and `LayerRaster::unseeded(resample_rgba_with_context(..))`.
  The 16-bit arm (`LayerRaster::new` of the quantized render) is untouched.
- `live_filter_evaluate.rs`: `source8 -> Colored8` — a cache hit is
  `Raster(pixels)`, no live stack is `color8`'s answer as is, a filtered
  result binds the whole colored plane ONCE (the same bind as the Sample
  arm), runs the unchanged `filtered`, and stores and answers
  `LayerRaster::unseeded(..)` as `Raster`. `source16`'s 8-bit promotion runs
  `precision_conversion::promote_raster` (the runs of the asset;
  `promote_samples` keeps its signature, both over the private
  `promote_runs`, `i` running).
- `live_filter_cache.rs`: `FilteredPixels::Byte(LayerRaster)`, recorded as
  `inventory.pin(Arc::new(v.clone()))` + `v.record_resources(inventory,
  "live_filter_cached_pixels", true)`.
- `live_filter_preview.rs`: `Preview::point(x, y)` (the clipped cell's
  centre; `index` is `point.1·w + point.0`, unchanged arithmetic); the byte
  arm reads `Colored8::sample_at`, binding no view.
- `doc_retained_source.rs` Place: the placeholder `rendered` is the asset's
  raster clone (`LayerRaster::unseeded(1×1)` for a 16-bit asset).
- `native_sources_assets.rs`: the asset PNG writer binds
  `pixels.contiguous_image()` once per asset (a 13B-2c-3 persistence read).

**Views an op leaves** (after `evict()`): a place with a matching or a
converting profile → 0; a filtered edit → the asset's one view (matching) or
0 (converting); a Sample-arm re-render → the colored raster's one view
(matching) or 0 (converting); the preview → 0 new; a `.rz` save → the
`rzdc.rs` per-layer bind and the asset writer's bind (13B-2c-3's list).

**Report walk** (`resources.rs`): the retained source is recorded BEFORE
`layer.pixels`, so a placed layer's tiles land under
`embedded_source_pixels` and dedup under `layer_pixels`; `tile_structure`
now also carries the asset indices.

## 13B-2c-3a contract

Eleven of the 25 residual `contiguous_image()` call lines are gone; the
residual is **14**, every one on 13B-2c-3b's list. Core-only: no FFI, header,
Swift, `.rz`, PSD, report-key or `schema_version` change; every `.rz`
stream, PSD stream and pixel output unchanged byte for byte; "46 first,
`LIDS` last" untouched.

- `layer_raster.rs`: `LayerRaster::runs(&self) -> impl Iterator<Item =
  &[[u8; 4]]>` — every row's runs in plane order, `row_segments(y, 0,
  width)` for `y` in `0..height`; no allocation, no view.
  `precision_conversion::promote_raster` and `retained_source_render::color8`
  walk it (their inline `flat_map` gone; `promote_runs` / `convert8_runs`
  unchanged, `i` running).
- Writers. `rzdc.rs` encodes each layer PNG from `layer.pixels.to_contiguous()?`
  (an owned transient dropped right after the encode; `Err` is the writer's
  `ALLOCATION` sentence); `native_sources_assets.rs` encodes each byte asset
  PNG from `pixels.to_contiguous()?` inside the unchanged `job.check()`
  bracket; `warp_native::put_raw_raster(out, offset, raster)` writes
  `put_raw`'s header with `len = w·h·4` and then the runs' bytes (`put_raw`
  stays for the mask). Bytes are identical by construction: a tiled copy is
  exactly `w·h·4`, a contiguous one is the raw buffer (the `samples()` rule
  `equals_raw` states).
- `psd_export.rs`: `component_planes(runs) -> [Vec<u8>; 4]` splits one pass
  into the record's channel order `[alpha, r, g, b]`; the unbaked arm feeds
  `layer.pixels.runs()`, the baked arm `once(bake.pixels.as_raw()
  .as_chunks::<4>().0)`; `w`, `h`, `(ox, oy)` as before.
- `doc_preservation.rs`: both `flat_save_losses` scans walk
  `l.pixels.runs().flatten()`; sentences and order unchanged.
- `ffi_doc.rs` `rz_doc_layer_thumbnail`: a raster reads
  `layer.pixels.to_contiguous().ok()?`, a group `layer_canvas_image`, into one
  owned `RgbaImage`; `ops::resize` unchanged; the `Cow` (and its `use`) is
  gone; NULL now also on an allocation failure (doc updated).
- `doc.rs` `layer_canvas_image`: `layer_rect::canvas_clip` → `None` answers
  the transparent canvas; else each clipped row's `row_segments` runs are one
  `copy_from_slice` each at canvas `(off_x + rx, off_y + ly)` (i64). A group
  still answers `group_projection` first. `inpaint_source.rs` `copy_layer`
  is the same walk with `control.check()?` once per row; the 256 KiB step
  loop is gone (a legal row is ≤ 30000 px = 120 KB, so it never stepped
  twice).
- `precision_conversion_structure.rs`: both promotions (`:400` the layer,
  `:525` the WARP original) run `promote_raster`; `promote_samples` stays
  for its other caller.
- Readers. `rzdc_read.rs` builds each decoded layer `LayerRaster::unseeded`
  and `warp_native::attach_byte` builds a decoded WARP original `unseeded`
  (the `equals_raw` adopt arm unchanged): an open seeds no view.
- **Reopen dedup** (`native_sources_byte::materialize_byte`): per binding,
  BEFORE the `RetainedSource8` is built, a layer whose bytes are its 8-bit
  asset's (`same_bytes`, dimensions first) becomes `asset.reidentified()`,
  so `rendered: layer.pixels.clone()` keeps `effective_source`'s `ptr_eq`
  and the layer IS the asset's plane under a fresh identity — the place-time
  figures. A 16-bit asset, or a live-filtered, scaled or edited layer, fails
  `same_bytes` and keeps its decoded plane. `attach_byte` still runs after.
- Unchanged signatures: `materialize_byte`, `layer_canvas_image`,
  `copy_layer`, `flat_save_losses`, every `rz_*` export. `doc_export.rs` and
  `source_asset.rs` were already off the residual (`doc_export.rs:935` reads
  an `RzImage`): nothing changed there.

**Views an op leaves** (after `evict()`): a `.rz` save, a PSD export, a
flat-save loss check, a thumbnail, a canvas image, a structure promotion, a
content-aware fill (its plate), a `.rz` reopen (plain, placed, dabbed or
bent) and a resave → 0. Temp-document reads left to 13B-2c-3b
(`doc_content_aware.rs:163`, `doc_retouch_output.rs:317`) are pruned when
their plane drops, so they never count after the call returns.

**Residual** (`rg -n 'contiguous_image\(\)' core/src`, 14 call lines, no
comment hits): `live_filter_evaluate.rs:346`, `ffi_retouch.rs:195`,
`doc_content_aware.rs:163`, `precision_scoped_source.rs:208`,
`doc_transform.rs:597`, `:601`, `retained_source_render.rs:311`,
`doc_geometry_set.rs:161`, `doc_warp.rs:76`, `doc_perspective.rs:329`,
`live_filter_profile.rs:122`, `doc.rs:1512`, `doc.rs:1709`,
`doc_retouch_output.rs:317` — all on 13B-2c-3b's row.

## 13B-2c-3b contract

The last 14 production `contiguous_image()` call lines are gone; the
residual is **0** and 13B-2 is closed. Core-only: no FFI, header, Swift,
`.rz`, PSD, report-key or `schema_version` change; every pixel output
byte-identical (the suite's contiguous twins and the unchanged pixel-golden
suites).

- **Tap-walk inputs → one transient copy.** `resample_into` / `map_into` /
  `resize_into` / `imageops` read the whole source at random 2-D offsets, so
  each site binds ONE owned `RgbaImage` at the top of its block and drops it
  with the block: `to_contiguous_with_job(job)?` where a `JobContext` is in
  hand (`doc_geometry_set.rs`'s plain arm, `doc_warp::render_pixels`,
  `retained_source_render::render8`'s Sample arm `held`,
  `live_filter_evaluate::source8`'s `held`), `to_contiguous().ok()?` in the
  `Option` paths (`doc_transform.rs`'s exact `Geom` arm and resample arm,
  `doc_perspective.rs`, `doc.rs`'s `geometry` — whose `collect` is now
  `Option<Vec<_>>` — and `resize`). An allocation failure answers `None` /
  `Err(ALLOCATION)` where it used to panic inside `contiguous_image`. No new
  checkpoint: the per-tile poll is 13B-1's measured `T_f + c_tile`.
- **Sequential walks → `runs()` / `row_segments`.**
  `live_filter_profile::convert`'s plain arm reserves
  `reserved_vec(byte_len)` and walks `runs().flatten()` with
  `job.every(i)` (its `rebuilt` output unchanged).
  `rz_doc_expand_retouch_destination` copies each source row's
  `row_segments(y, 0, lw)` runs with one `copy_from_slice` each at
  `((y+dy)·W + x+dx)·4`, `job.check()` once per row (a row ≤ 30000 px =
  120 KB, the 13B-2c-3a `copy_layer` rule).
- **Per-sample reader → `sample_at`.** `precision_scoped_source.rs`'s byte
  layer closure maps through the new `mapped_xy` (`mapped_index` is now
  `mapped_xy(..).map(|(x, y)| y·w + x)`) and reads `layer.pixels.sample_at`.
- **Temp documents.** `RzDocument::from_raster(pixels)` and
  `Layer::from_raster(pixels, name)` (`pub(crate)`; `from_pixels` /
  `Layer::new` delegate with `LayerRaster::new`, same field values). Both
  retouch temp documents are `from_raster(LayerRaster::unseeded(plate))`.
  `doc_content_aware`'s new-layer lift zips
  `healed.layers[0].pixels.runs().flatten()`; `deposit_into` takes
  `healed: &LayerRaster`, guards `healed.dimensions() == (W, H)` and reads
  only the domain rect through `read_rect`.
- **Outputs `unseeded`** at the eight geometry producers (`doc_transform`
  ×2, `doc_geometry_set`, `doc_perspective`, `doc.rs` `geometry` / `resize`,
  `doc_warp::render_pixels`, the retouch expansion): no production reader
  binds a view after this run. The `rebuilt` / `with_layer_pixels` whole-
  image writers stay seeded (13B-2b's decision).
- `layer_raster.rs`: doc comments only (`contiguous_image` has no production
  caller; the cache's read path, kept for the test oracles and 13B-6).
- Carried nits: `psd_export::component_planes(pixels, runs)` sizes each plane
  once (`Vec::with_capacity(pixels)`, no per-run `reserve`);
  `layer_raster_persistence.rs` (i) drops the vacuous `shared == 0` check;
  `layer_raster_writers.rs`'s `Site::views` is deleted (every site asserts
  `views == 0`). Flake: `projective_source_native.rs`'s `reject_bad_table`
  guards on the error with the file path replaced by `<path>` (the panic
  still quotes the raw error).
- Unchanged: every `rz_*` export, `resample_rgba[_with_context]`,
  `warp_into`, `sampling::resize`, `Geometry::apply`,
  `scoped_statistics::evaluate`, `LayerRaster`'s public surface, the
  `Colored8::Owned` arms, `AffineSampling::Shared`, `into_raster`.

**Views an op leaves** (after `evict()`): a layer transform (exact, resample
or translate), a perspective, a geometry set (plain or placed), rotate /
flip, resize, a warp, a retouch expansion, a scoped layer analysis, a live
filter edit and a heal onto a new layer → 0; a live-filtered profile
conversion → exactly its outputs' `rebuilt` seeds; a content-aware fill onto
a new layer → exactly its output's `with_layer_pixels` seed.

**Residual** (`rg -n 'contiguous_image\(' core/src`): the definition line
`layer_raster.rs:290` and doc-comment mentions only — 0 call lines.

## 13B-3a contract

`PrecisionImage` gets the accessor contract `LayerRaster` got in 13B-2a,
with the representation still `enum Pixels { U16(Arc<Vec<[u16; 4]>>),
F32(Arc<Vec<[f32; 4]>>) }`, so 13B-3b's tile flip is local to
`precision_image.rs`. Core-only: every pixel output, `.rz` / PSD byte, FFI
signature (`rz_precision_image_pixels_rgba16` / `_rgba32` answer the same
borrowed pointer; `RasterCore.swift` untouched), report key and
`schema_version` unchanged.

- **Removed**: `as_rgba16(&self) -> Option<&[[u16; 4]]>` and
  `as_rgba32(&self) -> Option<&[[f32; 4]]>` — no borrowed-slice accessor
  survives (`rg -n 'as_rgba16\(|as_rgba32\(' core/src core/tests
  core/examples` is empty).
- **Added** (`precision_image.rs`):
  - `contiguous_rgba16() -> Option<Arc<Vec<[u16; 4]>>>` /
    `contiguous_rgba32() -> Option<Arc<Vec<[f32; 4]>>>` — the bind-once
    whole-image view, `None` for the other storage. In 3a a clone of the
    image's own `Arc`, so `view.as_ptr()` (`Vec::as_ptr` through `Deref`)
    is the sample buffer.
  - `to_rgba16() / to_rgba32() -> Result<Vec<_>, String>` — owned copies
    through `reserved_vec` ("could not allocate precision output"); the
    other storage answers "precision image storage is not RGBA16" /
    "precision image storage is not RGBA32 float".
  - `ptr_eq(a, b)` — allocation IDENTITY: the same storage AND
    `Arc::ptr_eq` of the pixel `Arc`s (a clone is true; equal samples in two
    allocations false; u16 vs f32 false).
  - `same_samples(&other)` — sample EQUALITY with no allocation and no job:
    false on a storage or dimension mismatch, an `Arc::ptr_eq`
    short-circuit, u16 `==`, f32 per component through
    `tile_store::Sample::same_bits` (`-0.0` differs from `0.0`; the latch
    3b's tiles use).
- **Unchanged**: the five constructors, `dimensions`, `profile`,
  `with_profile`, `storage`, `storage_bytes`, `retained_bytes`,
  `record_resources` / `record_cache_resources` (categories
  `precision_buffer` / `precision_pixels` / `profile`), `pixel(index)`,
  `convert`, `to_rgba8`. `from_rgba32` / `from_rgba32_extended` keep their
  statement order verbatim — `out` → `admit` → `job.reserve` → the
  `every(i)` validation loop (over a view bound once on the line before the
  `for`) → `report(1.0, 5)` — so a NaN buffer under a sub-bookkeeping
  budget still answers the reservation refusal. `to_rgba8` gates on
  `storage() == Rgba16Unorm`.
- **Migration by site class** (each keeps its refusal sentence and its
  statement order):
  - storage GATES (`is_none()` at `artwork_document` ×2, `develop_inspect`,
    `precision_codec`, `precision_cutout`, `precision_paint_tools`,
    `raw_source_geometry`, `precision_scalar_edit`, `selected_adjustment`)
    → `storage() != Rgba16Unorm`, no view bound;
  - IDENTITY: `retained_source::effective_source` → `None` unless both
    images are u16 and `PrecisionImage::ptr_eq(image, &source.rendered)`
    (exactly where the old `?` / pointer test answered `None`);
    `precision_warp::same_image` → `a.storage() == Rgba16Unorm &&
    PrecisionImage::ptr_eq(a, b)`;
  - EQUALITY: `artwork_document`'s no-op test and `warp_native`'s
    adopt-existing-source test → `same_samples` (the `dimensions()` test
    beside each stays). `precision_geometry`'s polled comparison keeps its
    `job.check()` per 4096 samples and its two `a.as_ptr() == b.as_ptr()`
    short-circuits, over two views bound once (3c rewrites it over runs);
  - READS: every other site binds ONE view at the top of its function or
    block, or once per layer / per binding in a per-layer loop, and uses
    `&view` / `view.iter()` where a slice / iterator is needed;
  - the one PER-SAMPLE reader, `precision_style_render::StyledPixels::alpha16`,
    now reads `(image.pixel(index)[3] · 65535).round()` for both storages
    (in f32 `round(v/65535 · 65535) == v` for every u16 code — checked for
    all 65536 — so the raster answer is still the stored code);
  - FFI: the two pointer exports bind the view inside `handle_get` and lend
    its `as_ptr()` — valid because the view is the image's own `Arc`; a
    comment names 13B-3b's obligation (the handle must own the view it
    lends once tiled).
- **Tests and the example.** A mechanical rename; image-to-image identity
  oracles → `PrecisionImage::ptr_eq` (a captured pointer of an image became
  a captured image clone); the raw / `usize` address oracles keep
  `contiguous_rgba16/32().unwrap()/.expect(…).as_ptr()` in exactly six files
  (`precision_prototype` — the caller's `Vec` pointer before adoption —,
  `precision_canvas_rotation` 661–697, `precision_straighten`,
  `float_exact_geometry`, `placement_insert`, `retained_sources`
  878–883/912/988–993): the same fact in 3a, a 3b obligation;
  `.unwrap()/.expect(…).to_vec()` → `to_rgba16/32()`; `Option<view> ==
  Option<view>` compares samples as before; a view compared against a
  `Vec` / slice / array takes `*view` (deref to the `Vec`), a slice
  argument `&view`, an iterator `view.iter()`; the seven test helpers that
  returned `&[[u16; 4]]` now return an owned `Vec` through `to_rgba16()`
  (`precision_geometry.rs`'s `source()` was also an identity oracle through
  `.as_ptr()` at three sites — those now use `ptr_eq` on the layers).
- **Blast-radius recount** (HEAD `e7639f0`, `git archive` scratch copy):
  `core/src` 52 files / 94 lines (99 `as_rgba(16|32)(` matches including the
  two definitions); `core/tests` 93 files / 498 lines / 518 matches (plus 5
  path-form `PrecisionImage::as_rgba16` uses); `core/examples` 1 file / 2;
  identity-oracle chains 31 files / 70 (19 files / 33 single-line), of which
  6 files hold raw-address oracles; `.unwrap().to_vec()` 37 chains (12
  single-line), `.expect(…).to_vec()` 9; typed construction sites
  (`from_rgba16/32[_extended](`) 58 matches in 38 `core/src` files,
  untouched (58 / 38 after).
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 78 lines in
  49 files = the 2 definitions + **76 call lines**: `develop_inspect` 1,
  `doc_export_layers` 2, `doc_export` 2, `ffi_evaluation` 1, `ffi_float` 1,
  `ffi_precision` 1, `ffi_retouch` 1, `float_document` 1, `float_io` 1,
  `float_native` 1, `float_stats` 2, `live_filter_evaluate` 1,
  `live_filter_preview` 1, `precision_adjustment` 2,
  `precision_clipboard_codec` 2, `precision_clipboard_convert` 2,
  `precision_clipboard` 3, `precision_codec` 1, `precision_color_policy` 2,
  `precision_color` 2, `precision_conversion_narrow` 2,
  `precision_conversion_structure` 1, `precision_cutout` 1,
  `precision_geometry` 3, `precision_image` 4 (2 definitions + the two
  constructor validation views), `precision_inpaint` 1, `precision_lock` 4,
  `precision_mask_apply` 1, `precision_paint_tools` 1, `precision_paint` 3,
  `precision_plane_read` 1, `precision_quantize` 2,
  `precision_retouch_source` 1, `precision_retouch` 2, `precision_sampling`
  3, `precision_scalar_edit` 2, `precision_scoped_source` 1,
  `precision_selection` 1, `precision_stats` 2, `precision_warp` 1,
  `psd_export16` 2, `raw_local_develop` 2, `raw_source_geometry` 1,
  `retained_source_render` 1, `selected_adjustment` 1,
  `selected_auto_levels` 1, `selected_filter` 1, `soft_proof` 1,
  `warp_native` 1.

## 13B-3b-1 contract

`PrecisionImage` gets 13B-2b's representation, with every signature, pixel
output, `.rz` stream, FFI signature, report key and `schema_version`
unchanged. 13B-3b was SPLIT at its spec: this run is 13B-3b-1; `rebuilt` /
`with_rect` / `unseeded` / `reidentified` and their first callers are
13B-3b-2.

- **Representation** (private, `precision_image.rs`): `enum Plane<S> {
  Tiled(TilePlane<S>), Contiguous(Arc<Vec<S>>) }`, `enum Pixels {
  U16(Plane<[u16; 4]>), F32(Plane<[f32; 4]>) }`, plus `identity: Arc<()>`
  (minted by every constructor) and `pinned: OnceLock<Pinned>`. `Clone` is
  manual: pixels, profile and identity are shared, `pinned` starts EMPTY.
- **`tile()`**: every constructor builds the image Contiguous over the
  adopted `Arc<Vec<_>>` with 13B-3a's statement order VERBATIM (`from_rgba*`:
  `out` → `admit(out.retained_bytes(), …)` → `job.reserve` → the `every(i)`
  loop over the Arc'd Vec, no view bound → `tile()` → `report(1.0, 5)`;
  `from_float_samples` / `from_rendered_float`: their loops → `tile()` → the
  existing `job.check()`), then tiles it with `TilePlane::from_contiguous(w,
  h, &vec, None)` and SEEDS the view cache with that very `Arc<Vec>`. Tiling
  takes no job (no new checkpoint anywhere), a refused buffer never tiles or
  seeds, and identity is unchanged by it. A plane whose view the cache would
  refuse (`w·h·bpp > CONTIGUOUS_CACHE_BYTES`, i.e. f32 above 67,108,864 px;
  u16 never, 100 MP·8 = 800 MB) or that the tile store refuses stays
  Contiguous with no seed.
- **Accessors over both arms**: `contiguous_rgba16/32` — Tiled: the cache's
  view (`contiguous_views::view(PlaneKey::Rgba16/32(weak_index), ||
  to_contiguous(None).expect("could not allocate precision output"))`),
  Contiguous: `Arc::clone`; `to_rgba16/32` — Tiled `to_contiguous(None)`
  mapped to "could not allocate precision output", Contiguous 3a's copy;
  `ptr_eq` — same storage AND `Arc::ptr_eq(identity)` (clone, `with_profile`
  and a same-storage `convert` are true; a second construction false);
  `same_samples` — dimensions, then Tiled/Tiled `TilePlane::same_bits`,
  Contiguous/Contiguous 3a's, mixed → the tiled side's `equals_contiguous`;
  `pixel(index)` — Tiled `sample_at(index % w, index / w)` (out of range
  still panics), Contiguous `p[index]`; `storage_bytes` — Tiled `w·h·bpp`,
  Contiguous capacity-based; `record_storage_resources` — Tiled: the index as
  `tile_structure` and every tile as `precision_pixels` (no
  `precision_buffer`), plus a pinned clone of the image when `cache`;
  Contiguous: 3a's two records; a pinned view as `precision_views`; the
  profile as before.
- **The FFI lend**: `pinned_rgba16/32() -> Option<&[[u16|f32; 4]]>` binds
  the view through the cache ONCE, stores it in `pinned` and answers the
  stored view afterwards (an f32 image answers `None` from the u16 one
  without touching the slot, and the converse). `rz_precision_image_pixels_rgba16`
  / `_rgba32` lend `.as_ptr()` of it, so the pointer lives exactly as long as
  the HANDLE whatever the cache evicts; NULL on the other storage or on an
  allocation panic (`handle_get`'s `catch_unwind`). The header comment says
  so; no signature changed and `RasterCore.swift` is untouched. `pinned_*`
  is a lend, never a `core/src` read path.
- **`plane_key()`** (`pub(crate)`): Tiled — the index address
  `TilePlane::record_resources` records as `tile_structure`; Contiguous — the
  Vec's buffer. `precision_color`'s two address-keyed dedups key on it: the
  preparation walk gates on `storage() != Rgba16Unorm` (the existing
  sentence) and charges `w·h·8` per unique plane with no view; the
  conversion walk copies through `to_rgba16()?` instead of a view +
  `reserved_vec` + `extend_from_slice`.
- **`contiguous_views.rs`** (new): 13B-2b's cache moved verbatim out of
  `layer_raster.rs` and generalized — `PlaneKey::{Rgba8, Rgba16, Rgba32}`
  (a `Weak` of the plane index; `same` needs the same variant), `View::{Rgba8,
  Rgba16, Rgba32}` (`bytes()` is capacity-based), `seed`, `view` (hit, else
  materialize OUTSIDE the lock, re-check, insert), `contiguous_view_bytes`,
  `contiguous_views_of`, `record_contiguous_views`, `evict_contiguous_views`,
  and the two bounds (1 GiB / 256 entries, shared by byte and typed views).
  `layer_raster.rs`'s `seed` / `view` are `PlaneKey::Rgba8` / `View::Rgba8`
  wrappers and it re-exports `contiguous_view_bytes`,
  `CONTIGUOUS_CACHE_BYTES` and `CONTIGUOUS_CACHE_ENTRIES`, so the eight
  `layer_raster_*` suites are untouched; `resources.rs` calls
  `crate::contiguous_views::…`.
- **Report**: `precision_pixels` stays `w·h·bpp` per unique tiled plane;
  `tile_structure` now carries typed indices; `precision_buffer` appears only
  for a contiguous fallback; `precision_views` only after a pin (retained:
  the pinned view merges over its own cache entry through `Inventory::record`);
  typed views land in the same `contiguous_views` / `contiguous_view_bytes`,
  so `RasterResources.swift`'s trim rule needs nothing. `evict_caches` drops
  typed views too; its `scope` string is unchanged and `removed_cache_bytes`
  still counts only the supplied byte documents' views (a 13B-6 nit).
- **`work_plan`**: `scratch_bytes` gains `n·4·depth`, the TILING TRANSIENT —
  the rendered plane's tiles beside its contiguous render (`LayerRaster::new`
  / `unseeded` at 8 bits, the typed constructors' `tile()` at 16), charged
  whether or not the arm tiles, like the conservative source colour visit.
- **Tests and the example**: the six raw-address test oracles are
  `PrecisionImage::ptr_eq` now (pairwise over zipped layers where a
  `Vec<usize>` was compared, the same expectation at every site); the mask
  and channel address oracles beside them are 13B-4's and stay. The adoption
  asserts (`precision_image_accessor`, `precision_prototype`) are gone — the
  seed fact lives in the new `precision_image_tiles` suite — and
  `precision_prototype`'s 1024-capacity 1×1 image answers `storage_bytes()
  == 8`. `phase13b_tile_sharing` gains `typed/dab`, `typed/projection` and
  `typed/save`.
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 78 → **74
  lines = 2 definitions + 72 call lines** (`precision_color` −2, the two
  constructor validation views −2, the two FFI exports −2, the two
  `pinned_*` bodies +2).

## 13B-3b-2 contract

`PrecisionImage` gets 13B-2b's WRITER contract, with every pixel output,
`.rz` stream, FFI signature, report key and `schema_version` unchanged, and
`precision_image.rs` is split back under the soft cap.

- **The split** (`#[path]` CHILD modules, the `precision_geometry.rs:13-20`
  / `rzdc.rs:17-21` pattern; `lib.rs` untouched): `precision_image.rs`
  keeps the module doc, the consts, `PrecisionStorage`, the struct (its six
  fields PRIVATE), `Clone`, `pixel_count` / `admit` / `extended_pixel` /
  `reserved_vec`, every accessor, `convert`, `to_rgba8` and the new
  `shared_tile_count` (439 lines); `mod plane` (`precision_image_plane.rs`,
  238) holds `Plane<S>`, `Pixels`, `Pinned` and `tiles_of` (moved verbatim,
  now `pub(super)`) plus the plane-level `rebuilt_tiles` / `with_rect` /
  `read_rect` helpers; `mod build` (`precision_image_build.rs`, 485) holds
  `assembled`, `tile`, the five constructors and every writer. The
  representation is visible to `precision_image` and its two children only
  (compiler-enforced).
- **`from_rgba16_unseeded(w, h, pixels, profile, control)`**: `from_rgba16`'s
  statements verbatim with `tile_unseeded()` (the same `tiles_of`, no
  `contiguous_views::seed`).
- **`rebuilt_rgba16(&self, w, h, pixels, profile)`** /
  **`rebuilt_rgba32_extended`**: `from_rgba16` / `from_rgba32_extended`'s
  statements (length check, admission over the adopted buffer, `reserve`,
  the f32 `every(i)` / `extended_pixel` loop, `report`) with the tile step
  swapped for `rebuild_over(self)`: a same-storage TILED `self` with
  `(w, h) == self.dimensions()` → `TilePlane::rebuilt_from(&pixels, None)`
  (`Ok(Some(next))` → `next`, `Ok(None)` → `self`'s plane clone), and the
  adopted buffer SEEDS the view cache under the answered plane's key (a
  no-change rebuild seeds under the SAME key, which replaces the live entry:
  one view per plane); `Err` or any other `self` (other storage, other
  dimensions, contiguous) → `tile()` (fresh tiles, seeded; contiguous when
  refused). No `control` (all eleven sites passed `None`); ALWAYS a fresh
  identity. The constructors and the two `rebuilt_*` share one private body
  per storage (`adopted_rgba16` / `adopted_rgba32_extended`) that takes a
  `Tiling` policy (`Seeded` / `Unseeded` / `Rebuilt(&image)`), so the
  statement order the accessor and cancellation suites pin is written once.
- **`with_rect_rgba16/32(&self, x, y, w, h, samples)`**: `TilePlane::with_rect`
  semantics — `Err("precision image storage is not RGBA16")` (`… RGBA32
  float`) on the other storage, `Err` on a sample-count mismatch / a rect
  leaving the plane / an allocation failure, `Ok(None)` for a zero area or
  no bitwise change; `Some` = a fresh identity with the same profile, every
  untouched tile shared, NO seed. The f32 twin also refuses any sample
  failing `extended_pixel` with `from_rgba32_extended`'s sentence. A
  contiguous `self` takes the byte contiguous arm (compare bitwise, copy,
  write, `tiles_of` — above `CONTIGUOUS_CACHE_BYTES` it stays contiguous).
- **`read_rect_rgba16/32(&self, x, y, w, h)`**: `TilePlane::read_rect`
  semantics, row-major with stride `w`; `None` for the other storage, a zero
  area or a rect leaving the plane; a contiguous plane copies row slices. No
  view is bound. `with_rect` / `read_rect` land with their suite; their
  production callers (the typed rect writers) are 13B-3d's.
- **`reidentified(&self)`** (`pub(crate)`): the same samples, profile and
  representation under a fresh identity token, `pinned` empty.
  `retained_source_render::render_high_depth`'s `AffineSampling::Shared`
  arm answers `colored.reidentified()`, so a typed identity placement IS its
  master's tiles under its own identity (the byte `Colored8::into_raster`
  rule).
- **`shared_tile_count(&self, other)`** (`precision_image.rs`): tiles shared
  by pointer at the same index; 0 unless both are tiled with equal
  dimensions and storage. The test oracle.
- **Callers.** `rebuilt` (11): `precision_paint.rs:426` (`image`),
  `precision_paint_tools.rs:397` (the image `before` is bound from; `(nw,
  nh)` may differ — the fallback covers it), `precision_scalar_edit.rs:481`
  (`doc.layer_source(self.index)` bound once, then its view),
  `precision_mask_apply.rs:60` (`source`), `precision_retouch.rs:548`
  (`original`, in the `Layer` arm only — the construction moved into the two
  arms and `NewLayer` keeps `from_rgba16`), `selected_adjustment.rs:228`
  (`image`), `precision_lock.rs:306` / `:329` (`src`, f32 / u16),
  `precision_color.rs:358` (`image`), `precision_color_policy.rs:283`
  (`image`), `precision_clipboard.rs:483` (the Cut's `image`, f32). Every
  site's own `changed` scan, `job.every` loop, refusal order and no-op
  answer are verbatim. `unseeded` (8): `precision_conversion_structure.rs:386,
  395, 397, 521, 522`, `precision_conversion.rs:154`,
  `precision_conversion_narrow.rs:450`, `precision_retained_source.rs:268`.
- **Tests and the example**: the new black-box `precision_image_writers`
  suite (9); `layer_raster_persistence`'s promotion check is "promotion
  leaves no view" (`contiguous_view_bytes() == 0`, the promoted document's
  own views 0, no byte view — carried nit 1 subsumed); `precision_straighten`'s
  `rasters()` asserts u16 storage again (carried nit 2);
  `phase13b_tile_sharing` gains `typed/promote` and the
  `RZ_TILE_SHARING_ONLY=typed` filter.
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 74 = 2
  definitions + 72 call lines, unchanged (`precision_scalar_edit`'s bind
  moved to its own line).

## 13B-3c-1 contract

The typed twins of 13B-2c-1's byte read primitives land on `PrecisionImage`,
and the typed compositor, the regional typed tree and
`precision_geometry`'s polled comparison read through them. Every pixel
output (u16 codes, f32 bit patterns), `.rz` / PSD stream, FFI export, the C
header, `RasterCore.swift`, report key and `schema_version` is unchanged; no
checkpoint is added, removed or reordered; no `job` is passed where none was.

- **Plane layer** (`precision_image_plane.rs`): `PlaneRows<'a, S>` =
  `Tiled(tile_store::RowSegments)` | `Contiguous(Option<(u32, &[S])>)`;
  `Plane::row_segments(dims, y, x0, x1)` (tiled: `TilePlane::row_segments`;
  contiguous: ONE run `(x0, &p[y·w + x0 .. y·w + min(x1, w)])`, empty for
  `y >= h` or `x0 >= min(x1, w)`) and `Plane::sample_at(dims, x, y)` (`None`
  outside); `normalized16(sample)` = `f32::from(c) / 65535.0` per component,
  the ONE u16 read rule — `pixel()`, `sample_at` and `PrecisionRun::pixel`
  all go through it. Carried nit 2: `Plane::shared_tile_count`'s doc now says
  only the parent checks dimensions.
- **`PrecisionImage`** (`pub`): `PrecisionRun<'a>` = `Rgba16(&[[u16;4]])` |
  `Rgba32(&[[f32;4]])` (`Copy`; `len`, `is_empty`, `pixel(i)` = the value
  `PrecisionImage::pixel` answers, panicking out of range);
  `PrecisionRowSegments<'a>` (private inner `PlaneRows` per storage,
  `Iterator<Item = (u32, PrecisionRun)>`); `row_segments(y, x0, x1)`,
  `sample_at(x, y) -> Option<[f32;4]>`, `runs_rgba16()` / `runs_rgba32()`
  (`row_segments(y, 0, w)` for `y in 0..h`, `None` for the other storage).
  None of the four allocates or touches `contiguous_views`. The module doc's
  ACCESSOR paragraph and the `contiguous_rgba16` doc name them as the read
  path; `pixel(index)` stays only where an index is all a writer has
  (13B-3d's callers).
- **`render_layers`' `Kind::Raster` arm** (`precision_render.rs`): the
  `(x0..x1).step_by(4096)` chunk loop and `progress.advance(end − start)` per
  chunk are kept; each chunk reads `image.row_segments(ly, start − ox,
  end − ox)` (layer space, inside `[0, iw)` because `x0..x1` is
  `intersection`'s span) and feeds `run.pixel(j)` to
  `composite_raster_sample16` with the same `i`, `coverage` and `(x, y)`.
- **`StyledPixels`** (`precision_style_render.rs`): `pixel(index)` /
  `alpha16(index)` removed; `sample_at(x, y)` (Image → `image.sample_at`,
  Buffer → by index, `None` outside `width×height`), `alpha16_code(pixel)`
  (Image `(p[3]·65535).round()`, Buffer `p[3]·65535` — HEAD's `alpha16`
  bodies) and `visit_row(y, x0, x1, visit)` (plane columns `x0..min(x1, w)`
  in order; nothing when `y >= h` or `x0 >= x1`; the first `Err` stops the
  walk). `composite_pixels_into` and the knockout unit pass
  (`composite_interior_unit_into`) compute the layer-space span ONCE per layer
  (`(0, 0)` unless `x0 < x1`, 13B-2c-1's guard: no negative reaches a
  `u32`), keep the per-row `job.check()?` for every `ay in y0..y1` (even with
  an empty span) and run HEAD's statements inside `visit_row` (the unit
  pass's `continue` is `return Ok(())`). `typed_shape` walks
  `visit_row(y, 0, lw, …)` with `alpha16_code(px)` (`E = Infallible`).
- **Regional** (`precision_regional_tree.rs`, `precision_regional_style.rs`):
  `arm_sample` keeps its bounds test then reads `image.sample_at(sx, sy)`
  (`None` → the same `return Ok(())`); `direct_sample16`'s raster arm is
  `pixels.sample_at(sx, sy).map(|p| (p, pixels.alpha16_code(p)))`, the
  canvas-buffer arm `sample_at(xy.0, xy.1)` likewise, the 1×1 arm
  `Some((g[0], pixels.alpha16_code(g[0])))`.
- **`image_equal`** (`precision_geometry.rs`): dimensions first; both f32 →
  `a.plane_key() == b.plane_key()` short-circuits (HEAD's `as_ptr` equality:
  one cached view ⇔ one plane), else `runs_rgba32().flatten()` pairs zipped
  with `job.check()?` when `i % 4096 == 0` BEFORE comparing sample `i`,
  `Sample::same_bits`, `Ok(false)` at the first difference; otherwise both
  must be u16 (else HEAD's `"typed geometry requires RGBA16 storage"`), the
  same short-circuit and walk (`samples_equal`). One check per 4096-sample
  chunk reached — HEAD's sequence exactly. No view is bound.
- **Tests and the example**: `precision_image_reads` (new; (a)–(g) of the
  spec, (g) `#[ignore]`d) and `precision_compositor_parity` (new;
  HEAD-compilable, prints `parity <label> <depth> <sha256>`);
  `phase13b_tile_sharing`'s `typed/projection` gains
  `contiguous_view_bytes_document`, plus `typed/regional_window` and
  `typed/float_projection`.
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 71 = 2
  definitions + 69 call lines (`precision_geometry` −3).

## 13B-3c-2a contract

Every consumer of a typed projection handle reads the tiles; nothing it
reads binds or materializes a view. Every output (RGBA8 bytes, JSON
answers), refusal sentence and its order, checkpoint and report sequence
is HEAD `656a0aa`'s; no checkpoint is added, removed or reordered and no
`job` is passed where none was. No header, FFI signature, Swift, `.rz`,
PSD, report key or `schema_version` changes. The projection OUTPUTS stay
seeded (13B-3c-2b's item), so `precision_image_reads` (e) is unchanged.

- **`PrecisionImage::rgba16_at(x, y) -> Option<[u16; 4]>`** (new, `pub`):
  the STORED u16 code through `Plane::sample_at` — no normalization, `None`
  for f32 storage or outside, no allocation, no view. `sample_at` answers
  the normalized f32, so the code-exact readers need this one.
- **Quantization** (`precision_quantize.rs`): `quantize_scratch_bytes`
  gates `storage() != Rgba16Unorm` with the SAME sentence (`"byte
  quantization requires rgba16_unorm"`), then `BOOKKEEPING_BYTES + w·h·4`
  from the dimensions. `quantize_with_context` gates first, reserves
  `w·h·4`, and walks `y in 0..h` × `row_segments(y, 0, w)`: the flat `i =
  y·w + x` drives `job.report(i / n, 6)` exactly when `i & 4095 == 0`, the
  Bayer rank is `BAYER[(y & 7)·8 + (x & 7)]` (HEAD's `(i / w, i % w)`), the
  pushes are HEAD's; then `report(1.0, 5)`. Every caller (`to_rgba8` — the
  u16 canvas display, thumbnails, the patch / retouch overlays —,
  `display_rgba8`, `precision_conversion`, `precision_conversion_narrow`,
  `retained_source_render`, `artwork_document`, `doc_export`) is unchanged.
- **Tone map** (`float_document.rs`): `tone_mapped_u16` keeps the policy
  gate, then `runs_rgba32().ok_or("tone mapping requires rgba32_float
  storage")?` and feeds `runs.flatten().map(…)` (HEAD's closure) to
  `from_float_samples`. Its OUTPUT still seeds (13B-3c-2c re-classifies it).
- **Stats** (`precision_stats.rs`, `float_stats.rs`): `sample` /
  `float_sample` gate storage with the same sentence BEFORE the reach check
  (HEAD's order) and read `rgba16_at` / `sample_at` (f32 verbatim) for the
  neighbourhood and the centre. `histogram_checked` / `float_histogram`:
  `job.check()`, the storage gate, (u16) admission and reservation, then a
  TILE-BAND walk (each 256-row band, each 256-column span, each row's
  `row_segments` run; review fix — a row across every tile column cost
  ~20 % at 24 MP) sampling the flat indices that are multiples of `step`
  (each run starts at its first such index), with the report hoisted to
  each 4096-sample boundary: index `i = visited·step` (HEAD's
  `(0..n).step_by(step)` value) when `visited & 4095 == 0`. Every counter
  is order-independent and the reports read only `visited`, so answers,
  checkpoints and reports are the flat walk's. The ±0 minimum / maximum
  are order-independent only under arm64's `FMINNM` / `FMAXNM` (what
  `f32::min` / `f32::max` lower to), which order −0 < +0 — `f32::min`
  does not promise it; `precision_projection_consumers::
  record_float_histogram` pins it (13B-3c-2b, carried nit 1).
- **Soft proof** (`soft_proof.rs`): `proof_render16_with` builds the
  Proofer first, then the gate (same sentence), sizes bytes / mask by `n =
  w·h`, and walks `runs_rgba16().flatten().enumerate()` with `job.every(i)`
  and `mask[i]`.
- **Compare** (`ffi_evaluation.rs`): `rz_precision_image_compare` checks
  dimensions first, then needs `(a.runs_rgba16(), b.runs_rgba16())` both
  `Some` (else the same sentence), then walks both run streams with two
  cursors split at the shorter run and accumulates each pair's slice zip in
  nested loops into `Difference` — `difference_json`'s counters, split out
  so the byte compare keeps its iterator path (review fix: a zip of two
  flattened chains did not fold to a straight loop).
- **Scoped analysis** (`precision_scoped_source.rs`): after
  `analysis_source`, the gate `"analysis requires u16 document samples"`,
  then the closure `mapped_xy(i, …).and_then(|(x, y)|
  image.rgba16_at(x, y)).unwrap_or([0; 4])` — the byte closure's shape;
  the private `mapped_index` is deleted.
- **Carried nits**: (1) the summary's 13B-3c-1 figure now says "0, as at
  HEAD"; (2) `precision_regional_tree.rs`'s `let … else { return Ok(()) }`
  after `arm_sample`'s bounds test carries a comment saying it is
  unreachable.
- **Tests and the example**: `precision_projection_consumers` (new,
  HEAD-compilable; `record_*` print `consumer <label> <WxH> <sha256>
  checkpoints=<n> reports=<sha256>`; `no_view_*` fail at HEAD by design);
  `precision_image_reads` gains the `rgba16_at` test (b2);
  `phase13b_tile_sharing` gains `typed/consumers` (last in the full and
  `typed` runs, alone under `RZ_TILE_SHARING_ONLY=consumers`).
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 61 = 2
  definitions + 59 call lines in 38 files (−10 lines, 7 files emptied:
  `precision_quantize`, `float_document`, `precision_stats`, `float_stats`,
  `soft_proof`, `ffi_evaluation`, `precision_scoped_source`).

## 13B-3c-2b contract

Every typed render call site passes an explicit `OutputView`
(`precision_render.rs`, `pub(crate)`): `Seeded` — the adopted buffer seeds
the view cache, because the caller binds the output whole in the same
operation or installs it as a layer — or `Unseeded`, tiles only. It is
unrelated to `KnockoutSeed`, the "seeded" in `render_seeded_with_context`.
Every output (u16 codes, f32 bits), JSON answer, refusal and its order,
checkpoint and report sequence, and `.rz` / PSD / export byte is HEAD
`694194c`'s; no checkpoint is added, removed or reordered and no `job` is
passed where none was. No header, FFI signature, Swift, `.rz`, PSD, report
key or `schema_version` changes.

- **Constructors**: `from_float_samples_unseeded` /
  `from_rendered_float_unseeded` (`precision_image_build.rs`) run their
  seeded twin's statements verbatim through the private `Tiling`
  (`tile_unseeded()` in place of `tile()`), the `adopted_rgba16` pattern.
  `from_float_samples` / `from_rendered_float` stay seeded (other callers).
- **Plumbing**: `PrecisionScene::render_with_context(.., view, job)` picks
  the pair by `view`; `PrecisionScene::render` (public) is `Unseeded`. The
  regional window (`precision_regional.rs`) always uses the unseeded pair.
  `PrecisionDocument::render` (public, the host projection) is `Unseeded`;
  `render_with_context` / `render_cut_with_context` / `analysis_source` /
  `baked_projection` take `view`; `render_seeded_with_context` passes
  `Seeded` (its one caller, `precision_merge.rs:495`, installs a layer).
  `precision_io.rs:122`'s `self.render(limit, other, None)` became
  `render_with_context(limit, other, OutputView::Seeded, JobContext::NONE)`,
  keeping HEAD's seed for `precision_codec.rs:742`.
- **Moved readers** (so their renders can be `Unseeded`):
  - `Window::read` (`precision_clipboard.rs`, Copy and Copy Merged) matches
    on `image.storage()` (HEAD's unreachable "clipboard source has no typed
    samples" arm removed) and `Window::u16` / `f32` walk each window row:
    `job.check()` first, then in i64 the layer row `ly` and the clamped
    layer-space span `[lx0, lx1)` of `[a, b) = [x − offset.0, x + width −
    offset.0)`; the row is all zeros when `ly` misses the layer or `lx0 ≥
    lx1` (a row beside the layer), else `lx0 − a` zeros, the
    `row_segments(ly, lx0, lx1)` runs, zeros to `width` (debug-asserted);
    then alpha `cover_u16` / `cover_f32` per pixel; HEAD's `from_rgba16` /
    `from_rgba32_extended`.
  - The narrow preview gates `reference.runs_rgba16()` with HEAD's
    sentence, builds the after side (the `Rgba8` arm's ×257 `Vec` as
    `iter::once(slice)`; the `Rgba16` arm's render gated by
    `runs_rgba16()`, the `.to_vec()` gone) and walks both through the
    private `paired_runs` (two cursors split at the shorter run, zip
    semantics — `rz_precision_image_compare`'s pairing) with a flat `i` and
    `job.every(i)` per pair before HEAD's body.
  - The structure preview gates `after.runs_rgba16()` at HEAD's position and
    walks the runs with a flat cursor into `before.as_raw().chunks_exact(4)`
    (`job.every(i)` per sample, stopping at the shorter side).
  - `baked_projection` scans `runs_rgba16()` (HEAD's storage sentence) for
    any alpha below `u16::MAX`, then HEAD's refusal; no checkpoint.
- **Carried nits**: (1) `float_stats.rs`'s ±0 comment names the arm64
  `FMINNM` / `FMAXNM` assumption (−0 < +0; `f32::min` does not promise it)
  and the test that pins it (`precision_projection_consumers::
  record_float_histogram`, whose 600×520 fixture now puts blue +0.0 at
  (300, 10) and −0.0 at (10, 200) and asserts the stride-1 blue minimum is
  −0.0 bitwise); (2) `precision_image.rs`'s ACCESSOR paragraph re-wrapped
  to ~78 columns.
- **Tests and the example**: `precision_projection_unseeded` (new,
  HEAD-compilable; `record_*` print `unseeded <label> <WxH> <sha256>
  checkpoints=<n> reports=<sha256>`; `no_view_copy_layer` /
  `no_view_scene_render` fail at HEAD by design;
  `seeded_outputs_keep_their_view` passes at HEAD and pins D2);
  `precision_image_reads` (e) re-pinned at 0; `phase13b_tile_sharing`
  gains `typed/unseeded` (last in the full and `typed` runs, alone under
  `RZ_TILE_SHARING_ONLY=unseeded`).
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 55 = 2
  definitions + 53 call lines in 36 files (−6 lines, 2 files emptied:
  `precision_conversion_structure`, `precision_color_policy`).

## 13B-3c-2c-1 contract

13B-3c-2c is SPLIT at its spec: this run (13B-3c-2c-1) moves the whole-plane
readers of the retouch plates, the selection source, `precision_sampling`'s
resample inputs and `tone_mapped_u16`'s output off `contiguous_rgba16/32`
and classifies every constructor they feed; 13B-3c-2c-2 takes the other 19
constructor sites (the plan row). Every typed output (u16 codes, f32 bits),
JSON answer, refusal and its order, checkpoint and report sequence is HEAD
`549b10e`'s; no checkpoint is added, removed or reordered and no `job` is
passed where none was. No header, FFI signature, Swift, `.rz`, PSD, report
key or `schema_version` changes; the host readers of the plates
(`RetouchSourceOverlay.swift`, `PatchEdit.swift`,
`EditorViewController+PrecisionPaint.swift`, `EditorViewController+Heal.swift`)
read only `previewRGBA8` / `retouchTransformed` and are unchanged. The ONE
moved answer: an allocation failure in a transient copy answers Err "could
not allocate precision output" where HEAD's view materialization panicked
(13B-2c-3b's precedent).

- **The transient accessor**: `PrecisionImage::transient_rgba16` /
  `transient_rgba32` (`pub(crate)`) answer the whole plane for ONE
  random-access read per operation (a tap walk, a per-index closure) — a
  contiguous plane `Cow::Borrowed` from its own buffer (no copy; an f32
  plane above 67,108,864 px), a tiled one `Cow::Owned(to_contiguous(None))`.
  They never read or write the view cache and poll nothing (HEAD's
  materialization polled nothing either); Err "precision image storage is
  not RGBA16" / "… not RGBA32 float" for the other storage.
- **Storage gates**: every changed reader keeps HEAD's refusal as an
  explicit `storage()` gate with HEAD's sentence at HEAD's position (13B-3c-2a's
  rule), then reads.
- **Resample inputs** (`precision_sampling.rs`): `transform_image`'s float
  arm is gated by `storage() == Rgba32Float`, the Nearest refusal precedes
  `transient_rgba32()?`; its u16 arm and `resize_image` (after the
  equal-size short-circuit) refuse "typed geometry requires RGBA16 storage"
  then bind `transient_rgba16()?`. The tap closures read the transient.
- **The plates** (`precision_retouch_source.rs`): the Current arm refuses
  "retouch requires native u16 source", then per canvas row `job.check()`,
  `job.every(i)` for every canvas index of the row in order (HEAD's
  checkpoint sequence), then copies the layer row's `row_segments` runs
  under `Window::u16`'s column rule with the window the whole canvas (zeros
  where the layer does not reach) into `from_rgba16_unseeded`; the cut
  render passes `OutputView::Unseeded`.
- **The plate readers**: `precision_retouch.rs`'s backdrop ("retouch
  backdrop requires u16"; `debug_assert` plate dimensions == the extent)
  copies each row's runs into `pixels[y·W + x ..]`; its clone/heal source
  (None or non-u16 → "missing native retouch source") binds
  `transient_rgba16()?` for the bilinear tap walk; `precision_inpaint.rs`
  ("high-depth synthesis requires genuine u16 source pixels", before
  `job.reserve`) walks `row_segments(y0 + wy, x0, x0 + w)` per window row
  after `control.check()` with HEAD's per-sample body;
  `rz_precision_image_transform_retouch_source` ("native retouch source
  requires u16", after the reservation) binds `transient_rgba16()?` and
  answers `from_rgba16_unseeded` (its reader is the host's `previewRGBA8`).
  `precision_retouch.rs:368` (`before`, a whole-layer writer's input) and
  `:549` (the NewLayer output, Seeded) are 13B-3d's.
- **The selection source** (`precision_selection.rs`): `analysis_source(..,
  OutputView::Unseeded, ..)`, the gate ("native selection requires u16
  source"), then the private `canvas_samples(image, offset, (w, h))` —
  `w·h` row-major through `reserved_vec`, per canvas row `Window::u16`'s
  column/row rule over `row_segments` with the window the whole canvas, no
  coverage and no checkpoint — and `drop(image)` before the analysis, whose
  closure is `|i| canvas[i]`. `canvas_samples` holds one UNCHARGED
  canvas-sized `W·H·8` buffer across the analysis, so for a small Layer
  source on a large canvas it exceeds the layer-sized view HEAD read; no
  code change (charging it would change admission answers).
- **`tone_mapped_u16(image, view, job)`**: `Seeded` → `from_float_samples`,
  `Unseeded` → `from_float_samples_unseeded`, statements otherwise verbatim;
  the private narrow `leaf_u16` takes `view` from its two callers. The six
  call sites choose per the decisions table; `doc_export::export_stages`
  chooses `Seeded` iff `(w, h) == source.dimensions()`.
- **Carried nit**: `precision_projection_unseeded::record_narrow_preview`
  adds `narrow_preview/rgba16_fractional` — `extended(w, h, 5)` unmodified
  (alpha 0, 1, k/11), `skipped_latent_rgb == 3·hidden` and
  `compared_samples == 4n − 3·hidden` asserted (hidden = alpha 0.0), the
  deltas recorded only.
- **Tests and the example**: `precision_plates` (new, HEAD-compilable;
  `record_*` print `plates <label> <WxH> <sha256> checkpoints=<n>
  reports=<sha256>`; the four `no_view_*` fail at HEAD by design;
  `seeded_outputs_keep_their_view` passes at HEAD); `phase13b_tile_sharing`
  gains `typed/plates` (last in the full and `typed` runs, alone under
  `RZ_TILE_SHARING_ONLY=plates`).
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 47 = 2
  definitions + 45 call lines in 31 files (−8 lines, 5 files emptied:
  `precision_sampling`, `precision_retouch_source`, `precision_inpaint`,
  `ffi_retouch`, `precision_selection`; `precision_retouch.rs:427`'s
  path-form bind `PrecisionImage::contiguous_rgba16` is gone too).

## 13B-3c-2c-2 contract

This run classifies the other 19 typed constructor sites of 13B-3b-2's 29
(the plan row) and unseeds the two with no whole-plane reader left: the
Copy / Cut payload, after its two sequential readers move onto runs, and
`convert`'s cross-storage output. Every typed output (u16 codes, f32 bits),
RZCB and `.rz` byte, refusal and its order, checkpoint and report sequence
is HEAD `71e4873`'s; no checkpoint is added, removed or reordered and no
`job` is passed where none was. Admission is unchanged (Copy still charges
`2 · payload_bytes`). No header, FFI signature, Swift, `.rz`, RZCB, PSD,
report key or `schema_version` changes.

- **The constructor**: `PrecisionImage::from_rgba32_extended_unseeded`
  (`pub(crate)`) is `adopted_rgba32_extended(.., Tiling::Unseeded)` — the
  seeded constructor's statements, refusals and checkpoints with the seed
  dropped, the f32 twin of `from_rgba16_unseeded`.
- **The payload** (`precision_clipboard.rs`): `Window::u16` answers
  `from_rgba16_unseeded` and `Window::f32` `from_rgba32_extended_unseeded`.
  Cut shares `clipboard_copy_with_context`, so its payload is unseeded too.
  `RzClipboardPayload::new` (`ffi_clipboard.rs:29-31`) encodes when the
  handle is made, so at HEAD the payload kept its seed for the handle's
  whole life; now the handle holds tiles and the RZCB bytes only.
- **`encode`** (`precision_clipboard_codec.rs`) keeps HEAD's `if let … else
  if let …` shape over `runs_rgba16()` / `runs_rgba32()`, a plain nested
  walk per run over `run.as_flattened()` writing each component
  little-endian — the same bytes in the same order.
- **`converted_with_context`** (`precision_clipboard_convert.rs`) keeps its
  `if let / else if let / else Err("clipboard payload has no typed
  samples")` shape: the u16 arm extends the output by each run, the f32 arm
  builds the `FloatPolicy` first (HEAD's refusal position) and walks the
  runs with a flat counter `i`, `job.every(i)?` before HEAD's per-sample
  body, so the checkpoint sequence is HEAD's.
- **`convert`** (`precision_image.rs`): the cross-storage arm calls
  `from_float_samples_unseeded` with HEAD's arguments; the same-storage arm
  still shares the handle. `convert` has no `core/src` caller; its readers
  are the tests' owned copies.
- **Tests and the example**: `precision_constructor_seeds` (new,
  HEAD-compilable; `record_*` print `seeds <label> <WxH> <sha256>
  checkpoints=<n> reports=<sha256>`; the three `no_view_*` fail at HEAD by
  design; `seeded_outputs_keep_their_view` passes at HEAD);
  `phase13b_tile_sharing` gains `typed/clipboard` (last in the full and
  `typed` runs, alone under `RZ_TILE_SHARING_ONLY=clipboard`; `plates_line`
  is `timed_line` with the op label as a parameter).
- **The 9g probe reader** (`scripts/live-filter-probe/native.py`,
  script-only): `_identity_body` peels the 12E-1 outer word 45/46 — `LIDS`
  found by `rfind`, schema 1, count > 0, `lids + 12 + 8·count == len`,
  every id nonzero and unique — into `data[:4] + data[8:lids]` at offset 4;
  any other first word is read unchanged. `read_native` parses that body as
  before, asserts one id per record and shifts every recorded position (the
  live-filter binding range, both asset ranges, `resource_ranges`) back
  into the file, so `mutate_filter` splices the ORIGINAL file and a mutated
  45/46 file keeps its wrapper and `LIDS`. It answers `identity_version`
  (45 / 46 / None) and `layer_ids`; `version` stays the first word inside
  the wrapper. The words nested inside the wrapper (38–44, 47–54) are not
  taught; `prefix`'s assertion names them.
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 43 = 2
  definitions + 41 call lines in 29 files (−4 lines; `precision_clipboard_codec`
  and `precision_clipboard_convert` emptied).

## 13B-3d-1 contract

13B-3d is SPLIT at its spec (plan table): this run moves the typed PAINT
writers onto `read_rect` + `with_rect`, mirroring 13B-2c-2a's byte rect
writers — the rect bounds the READ, `with_rect`'s bitwise diff bounds the
copies and `with_rect` never seeds. Every pixel output (u16 codes, f32
bits), refusal (text and order), plan JSON and admission answer (`admit`,
`job.reserve` figures) is HEAD `97e5825`'s; only the walks' checkpoint and
progress sequence shrinks to the rect. No checkpoint SITE is added; the
rect walks keep ≤ 4096 pixel bodies (or one row) between checks. No FFI,
header, Swift, `.rz`, PSD, RZCB, report key, plan JSON or
`schema_version` change; no new `pub` item.

- **The plane forms** (`layer_rect.rs`, `pub(crate)`, allocation-free,
  layer-space `(x, y, w, h)`, `None` = empty): `plane_clip(canvas,
  dimensions, offset)`, `canvas_rect_on_plane(.., rect)` (a canvas rect with
  exclusive ends ∩ `plane_clip`), `coverage_rect16(coverage, canvas,
  dimensions, offset)` (the bbox of the canvas-frame samples > 0 inside
  `plane_clip`; with `dimensions = canvas, offset = (0, 0)` a canvas-space
  rect) and `plane_rect_offset(offset, rect)`. The byte `canvas_clip` /
  `canvas_rect_on_layer` / `coverage_rect` / `rect_offset` keep their
  signatures and delegate; `coverage_rect` and `coverage_rect16` share ONE
  private generic row scan.
- **`paint_plan`**: the target's `storage()` gate carries HEAD's sentence
  ("high-depth paint requires u16 storage") at the view bind's position and
  `output_pixels = pixel_count(w, h)`; the plan JSON is unchanged.
- **`painting_unlocked`** (Paint / Erase / Clone / Heal; the Rasterize arm
  untouched): HEAD's statements through `opacity == 0`, the layer / source
  lookups, then the storage gate. Paint / Erase / Clone: Clone builds its
  `SourceView` first (HEAD's refusal order), then `L =
  coverage_rect16(coverage, canvas, (w, h), offset)` — `None` answers
  `report(1.0, 5)`, `Ok(None)`; ONE `read_rect_rgba16(L)` buffer (None →
  "could not allocate precision output") is walked rows top to bottom with
  HEAD's per-pixel statements verbatim at rect-relative `k` (so
  `checked_alpha`'s first refusal is HEAD's), `job.check()` per row,
  `job.every(k)` per sample and `job.report((row + 1) / rh, 3)` per row,
  then `with_rect_rgba16(L, &buf)`: `Ok(None)` → `report(1.0, 5)`,
  `Ok(None)`; `Some(image)` → `replacing_content`, `report(1.0, 5)`. The
  whole-plane copy, its per-4096 checks and the changed scan are gone (the
  latch is `with_rect`'s). Heal: `L = canvas_rect_on_plane(canvas, (w, h),
  offset, window)`; `precision_heal::apply` runs over L's buffer with the
  plane `((rw, rh), offset + (x, y))`, then `with_rect` as above; a window
  missing the layer runs `apply` over an EMPTY buffer at `((0, 0),
  offset)` — every `layer_byte` is `None`, exactly HEAD's for such a
  window, so the solve, its checkpoints and errors are HEAD's — then
  `report(1.0, 5)`, `Ok(None)`.
- **`SourceView`** holds `&PrecisionImage` + offset (its storage gate
  keeps "high-depth source requires u16 storage") and reads ONE stored code
  per tap (`rgba16_at`) inside HEAD's `canvas_index` bounds.
- **`applying_unlocked`** (the five native tools; HEAD's order through
  `prepare_colors`): bucket fill grows its region over ONE
  `transient_rgba16` (13B-3c-2c-1's rule, dropped after `grow`; an
  allocation failure answers Err where HEAD's view panicked). The walk
  rect R is canvas-space: erase_native / dodge_burn take
  `coverage_rect16(&input[..n], canvas, (w, h), (ox, oy))` shifted by the
  offset (neither changes a sample off the layer), paint_blend /
  bucket_fill / gradient `coverage_rect16(&input[..n], canvas, canvas, (0,
  0))`; `None` → `report(1., 5)`, `Ok(None)`. R's buffer is the layer's
  `read_rect` of `canvas_rect_on_plane(.., R)` placed in `[0; 4]`; HEAD's
  per-pixel body runs verbatim at canvas `(x, y)` / `i = y·W + x` (input,
  selection, region, gradient and Dissolve coordinates unchanged), with
  HEAD's `bounds` / `changed`, `job.every(k)` and a report `0.3 +
  0.5·k/area` at `k & 65535 == 0`. `!changed` → HEAD. The extent E is
  HEAD's (`merged_extent` / the layer's): E equal to the layer → R ∩ layer
  back through `with_rect_rgba16`; a GROWN E (changed ink left the layer)
  keeps HEAD's `rebuilt_rgba16(nw, nh, ..)`, each row zeros, the old
  layer's `row_segments` runs, then R's values over `R ∩ E` only (R's
  samples outside E are unchanged off-layer zeros), one `job.check()` per
  row.
- **`erased_float`** (the float Cut's erase): the storage gate "a float
  Cut requires rgba32_float storage" at the bind's position, its scratch
  from `pixel_count(w, h)`, then `coverage_rect16`'s rect (`None` →
  `Ok(None)`), `read_rect_rgba32`, HEAD's body per rect row, and
  `with_rect_rgba32` → `replacing_content`. The u16 Cut erases through
  `paint_with_context` (above), so a Cut leaves no view at either depth.
- **Profiles**: every raster's profile is `Arc::ptr_eq` the document's
  (`precision_document.rs:505-516`), so `with_rect`'s inherited profile is
  HEAD's `rebuilt_*(.., self.profile.clone())`.
- **The canvas-proportional pass left** is the bbox scan of the caller's
  canvas-sized coverage (no plane read, no allocation, nothing polled);
  bounding it needs a rect-shaped FFI input — out of scope.
- **Tests and the example**: `precision_rect_writers` (new,
  HEAD-compilable, black-box through `rz_precision_doc_edit` /
  `rz_precision_doc_clipboard_cut`): `record_rect_writers` prints `rect
  <site>/<case> <WxH> <sha256>`; `no_view_rect_writers` and
  `dab_checkpoints_are_plane_size_independent` fail at HEAD by design,
  `rect_writers_share_untouched_tiles` and the record pass there. A GROWN
  answer (target dimensions or offset differ from the source's) is keyed
  by shape, never by case name. `precision_image_writers` gains the
  ignored `j_contiguous_fallback_rect_write_and_read` (carried nit 1);
  `precision_constructor_seeds`' Cut now asserts 0 views.
  `phase13b_tile_sharing`'s `typed/dab` gains `contiguous_view_bytes_base`,
  `ms_dabs`, `ms_median`, `checkpoints` and `sha256`, and `typed/dab_blend`
  paints the same dabs through `paint_blend` (`RZ_TILE_SHARING_ONLY=dab`
  prints only the two).
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 38 = 2
  definitions + 36 call lines in 26 files (−5 lines: `paint_plan`,
  `painting_unlocked`, `SourceView`, `applying_unlocked`, `erased_float`;
  `precision_paint`, `precision_paint_tools` and `precision_clipboard`
  emptied).

## 13B-3d-2 contract

13B-3d's part 2 (plan table): the remaining typed writers named in the
13B-3d-2 row bind no whole contiguous view. The in-place retouch arm and the
transparency-lock restore read and write ONE rect (`read_rect_rgba16/32` +
`with_rect_rgba16/32`, 13B-3d-1's pattern: the rect bounds the READ,
`with_rect`'s bitwise diff is the changed latch and bounds the copies,
`with_rect` never seeds); the whole-image writers read rows / runs and
answer through `rebuilt_*` (the destructive adjustment through the new
`rebuilt_float_samples`). Every pixel output (u16 codes, f32 bits), refusal
(text and order), plan JSON, `admit` / `job.reserve` figure and report
(progress and stage) is HEAD `a33e203`'s. The stated differences: the
in-place retouch path's checkpoint sequence shrinks (no extent buffer,
baseline or changed scan); its rect read's allocation failure surfaces after
the plate render; and a destructive adjustment of an f32 raster, which
panicked at HEAD ("internal error: applying high-depth adjustment"), refuses
by name ("destructive high-depth adjustment requires u16 storage"). No
checkpoint SITE is added. No FFI, header, Swift, `.rz`, PSD, RZCB, report
key, plan JSON or `schema_version` change; no new `pub` item.

- **`precision_retouch_apply.rs`** (new, private): `RetouchKernel` —
  `Sampled(Vec<[u16; 4]>)` (Clone / Heal), `Synthesis(InpaintPlan,
  Option<Vec<bool>>)` (Spot Heal / Content-Aware Fill), `RedEye` — holds
  HEAD's `match edit.op` split at the destination write. `prepare` is HEAD's
  statements before the write, verbatim and in order (the "missing native
  retouch source" gate, `transient_rgba16`, `retouch_transform::transform`;
  `bounds`, `estimate`, `precision_inpaint::plan(.., scratch − estimate,
  ..)`, `source_mask.compile`; nothing for red-eye). `apply` is HEAD's write
  with the plane `((w, h), canvas offset)` in place of `(dimensions,
  offset)` (Clone's per-pixel loop, `precision_retouch_heal::apply`,
  `precision_inpaint::apply` with "missing spot source",
  `precision_redeye::apply`). `bound` is the canvas rect (exclusive ends)
  the write reads or changes: Clone the bbox of `coverage > 0`
  (`coverage_rect16(coverage, canvas, canvas, (0, 0))`), Heal that bbox
  grown by 1 on every side (every component's padded window lies inside
  it), Spot Heal / Content-Aware Fill `InpaintPlan::window_union` (new,
  `None` without windows), red-eye `(x, y, x + width, y + height)`. Every
  write indexes the destination only through canvas coordinates (Clone's
  `canvas_index`, the membrane writeback's `layer_byte`, red-eye's rect
  `index`), so a plane covering every sample the write reads or changes
  answers HEAD's codes.
- **`retouching_unlocked`**: the `before` view bind becomes the storage
  gate "retouch requires u16 storage" at its position; `in_place =
  output == Layer && (extent) == (layer dimensions, layer offset)`. Not in
  place (a new layer, or an `expand_destination` extent that differs) keeps
  HEAD's path (`retouch_destination`: the extent buffer, the backdrop's
  rows, the Layer fill now from `original.row_segments` runs with HEAD's
  mapping and `job.every(y·w + x + k)`, a non-u16 run refusing "retouch
  requires u16 storage"; the baseline; then the plate, `prepare`, `apply`
  over the whole extent, the changed scan, the backdrop zeroing and
  `from_rgba16` / `rebuilt_rgba16`). In place: the plate, `job.reserve`,
  `edit_job.report(0.0, 2)` and `prepare` as at HEAD, then `L =
  bound ∩ layer` (`canvas_rect_on_plane`); `None` runs `apply` over an
  EMPTY plane `((0, 0), offset)` (HEAD's solve and refusals, nothing
  written) and answers `report(1.0, 5)`, `Ok(None)`; `Some(L)` reads ONE
  `read_rect_rgba16(L)` ("could not allocate precision output" on
  failure), runs `apply` over it at `plane_rect_offset(offset, L)` and
  writes it back through `with_rect_rgba16` — `None` → `report(1.0, 5)`,
  `Ok(None)`; `Some(image)` → `replacing_content`, `report(1.0, 5)`.
  `retouch_plan`, the domain / bounds / `any` scans and the plates are
  untouched. `RetouchEdit::bounds` is `pub(crate)`.
- **`precision_inpaint::apply`**'s destination length check reads
  `pixels.len() as u64 != w·h` (same sentence), so the empty plane passes.
- **`freezing_alpha`** (`precision_lock.rs`), after HEAD's dimension /
  offset gate: `src.storage()` is gated against `dst.storage()` with HEAD's
  sentences ("high-depth lock source is not float" / "… not u16"); `R =
  src.unshared_rect(dst)` — `None` → `(src.reidentified(), true)`;
  `Some(R)` → one `read_rect_*` of each ("could not allocate precision
  output"), HEAD's per-pixel statement and `same` over the zipped pair into
  `reserved_vec(R.w·R.h)`, `src.with_rect_*(R, &pixels)?` (`None` →
  `src.reidentified()`). Exact: in a shared tile `d == s` bitwise, so HEAD's
  pixel there is `s` and `same` holds. The latch (`unchanged &&
  same_mask`) and `replacing_layer` are unchanged; every `under_locks`
  caller (paint, the native tools, retouch, scalar edit, the destructive
  and selected adjustments, artwork, geometry, mask edit, the float Cut)
  reaches it. A `dst` sharing no tile restores over the whole plane.
- **`PrecisionImage::unshared_rect`** (`pub(crate)`, no allocation, no
  view, no job; `Plane::unshared_rect` in `precision_image_plane.rs`): the
  bbox of the `TilePlane::tile_rect`s whose tile `Arc`s differ, `None` when
  every tile is shared (or both hold the same contiguous `Arc`), the whole
  plane on a storage / dimension mismatch or any other contiguous pair.
- **`rebuilt_float_samples`** (`precision_image_build.rs`):
  `float_samples(.., Tiling::Rebuilt(self))` — `from_float_samples`'
  statements and checkpoints over the edited image's tiles, seeding like
  `rebuilt_rgba16`, always a fresh identity. `applying_adjustment_unlocked`
  builds its output with it and scans `changed` in TILE-BAND order (each
  256-row band, each 256-column span, each row: `output.row_segments(y, x0,
  x1)` zipped with `image.row_segments(y, x0, x1)` — a tile-aligned span is
  one run in either representation — then `job.every(i)?; changed |= a !=
  b; i += 1` on the visit counter, 13B-3c-2a's histogram walk; the review
  fix pass replaced the first cut's row-across-tiles cursor pair); a non-u16
  storage refuses by name.
- **The whole-image writers' binds** walk runs in plain nested loops (`let
  mut next = 0; for run in runs { for p in run { let i = next; next += 1;
  … } }` — never `.flatten()` / `.enumerate()` / `.zip()` over the run
  chain), HEAD's bodies, checkpoints and refusal order verbatim:
  `precision_scalar_edit`'s `read()` and RGBA writer ("native scalar color
  plane requires u16 storage"; after the review fix pass `read()` walks in
  TILE-BAND order into the pre-sized plane with `job.every` on the visit
  counter, and the writer does one `extend_from_slice` per run then ONE flat
  `job.every(i)?; pixels[i][c] = out[i]` pass), `precision_mask_apply` ("mask apply requires
  native u16 raster"), `selected_adjustment` (the `runs_rgba16` iterator
  bound where the view was, "selected edit requires u16 source"),
  `precision_color_policy::convert_pixels` (per layer, `i & 4095`
  checkpoints on the counter); buffers `reserved_vec(w·h)`.
- **`float_document.rs:620`** (`tone_mapped_u16`, f32 → u16) needs no
  rebuild twin: an output of another storage shares no tile with its
  source, so `Tiling::Rebuilt` would tile fresh anyway. The file is
  untouched.
- **Carried nit**: the native tools' grown rebuild
  (`precision_paint_tools.rs`) refuses a non-u16 run ("native paint
  requires u16 storage") instead of skipping it.
- **The canvas-proportional retouch passes left** (in place): the source
  plate (the Current arm copies the layer's rows onto a canvas-sized plate,
  13B-3c-2c-1), the source transform (a canvas-sized sampled plane), and
  the scans of the caller's canvas-sized coverage (the selection domain,
  the admitted-bounds check, `any`, and the new `bound` bbox) — no plane
  read and no view, but a 24 MP dab still pays them (13B-9 for the plate
  and the transform; the scans need a rect-shaped FFI input).
- **Tests and the example**: `precision_retouch_writers` (new,
  HEAD-compilable, black-box through `rz_precision_doc_edit`,
  `rz_precision_doc_selected_edit`, `rz_precision_doc_apply_adjustment`,
  `rz_precision_doc_convert_to_profile_with`,
  `rz_precision_doc_clipboard_cut`): `record_retouch_writers` prints
  `retouch <site>/<case> <WxH> <sha256>`; `no_view_retouch_writers` and
  `float_destructive_adjustment_refuses_by_name` fail at HEAD by design,
  `retouch_dabs_share_untouched_tiles` and the record pass there.
  `precision_rect_writers::no_view_rect_writers` covers the `locked` case;
  `precision_plates::no_view_retouch_clone_source_layer` expects 0 view
  bytes. `phase13b_tile_sharing` gains `typed/retouch_dab`,
  `typed/locked_dab`, `typed/scalar_dab` and `typed/whole_writers`
  (`apply_adjustment`), printed by `RZ_TILE_SHARING_ONLY=retouch` and at
  the end of the full run.
- **Residual** (`rg -c 'contiguous_rgba(16|32)\(' core/src`): 26 = 2
  definitions + 24 call lines in 19 files (−12 lines: `precision_retouch`,
  `precision_scalar_edit` ×2, `precision_lock` ×4, `precision_mask_apply`,
  `selected_adjustment`, `precision_color_policy`, `precision_adjustment`
  ×2; those seven files emptied).

## 13B-3d-3a contract

13B-3d-3 is SPLIT at its spec: 13B-3d-3a (this run) makes every Seeded typed
LAYER producer that can flip through an EXISTING unseeded constructor or
the `rebuilt_*` tiling policy leave no view; 13B-3d-3b (plan table) owns
the `TilePlane::filled` planes and the resample outputs. Every output (u16
codes, f32 bits), refusal and its order, checkpoint and report sequence,
`.rz` / PSD / RZCB byte and admission figure is HEAD `63421b3`'s: the
flips drop an output SEED and move no read, `Tiling` polls nothing, and
`retained_bytes` / admission never read the view cache — resource reports
show smaller `contiguous_view_bytes` only. No FFI, header, Swift, `.rz`,
PSD, RZCB, report key, plan JSON or `schema_version` change; no new `pub`
item; `OutputView` keeps both variants.

- **`rebuild_over`** (`precision_image_build.rs`, `Tiling::Rebuilt`): the
  two `contiguous_views::seed` calls are deleted and the fallthrough is
  `tile_unseeded()` (was `tile()`), so `rebuilt_rgba16` /
  `rebuilt_rgba32_extended` / `rebuilt_float_samples` share every
  bitwise-unchanged tile and seed NOTHING (always a fresh identity;
  signatures, length check, admission, validation and checkpoints
  unchanged). This flips all eight callers at once:
  `precision_scalar_edit.rs:508`, `precision_mask_apply.rs:65`,
  `selected_adjustment.rs:234`, `precision_color_policy.rs:289`
  (`convert_pixels`), `precision_color.rs:357`,
  `precision_adjustment.rs:305` (incl. the no-change answer),
  `precision_retouch.rs:455` (the grown extent) and
  `precision_paint_tools.rs:473` (the native tools' GROWN extent — new
  dimensions, so a whole rebuild from rows, no view).
- **Twelve constructor / render sites**: `precision_merge.rs:301/361/390`
  (merge, stamp, flatten) and `precision_document.rs:911`
  (`render_seeded_with_context`, the members render of
  `precision_merge::merged_pixels`) render `OutputView::Unseeded`;
  `precision_color_policy::baked_projection` loses its `view` parameter and
  always renders `Unseeded` (the preflight and `bake_adjustment` alike);
  `artwork_render.rs:142`, `precision_warp.rs:117`, `precision_paint.rs:305`
  (Rasterize), `precision_clipboard_convert.rs:123` and
  `precision_retouch.rs:446` (NewLayer) call `from_rgba16_unseeded` with
  the same arguments; `float_document.rs:690` (`float_to_u16`) tone-maps
  `Unseeded`; `precision_conversion_narrow::leaf_u16` loses its `view` and
  calls `tone_mapped_u16(image, OutputView::Unseeded, job)` for both
  narrowings.
- **Readers.** Every flipped output is INSTALLED as a layer (the artwork
  handle through `artwork_unlocked`'s `same_samples` and the byte publish's
  quantize rows; the bake read by `convert_pixels`' runs; the warp output
  also as the binding's `rendered_pixels`, compared by `effective_warp`'s
  `ptr_eq`). Its view-free readers are the rect / runs writers, the
  compositor, the regional window, thumbnails and histograms; its whole
  readers are the 13B-3e residual, each paying ONE materialization on its
  first read, then cached: `precision_codec.rs:742` (the typed native save
  via `precision_native.rs:508`), `float_native.rs:73`,
  `psd_export16.rs:363`, `precision_warp.rs:85` (a warp's first edit),
  `warp_native.rs:217`, `selected_filter.rs:263`,
  `selected_auto_levels.rs:52`, `precision_plane_read.rs:199`.
- **Unchanged** (13B-3d-3b / 13B-3e): the byte `LayerRaster::rebuilt` seed
  (13B-2b decision), `from_rgba16` and every other seeded constructor,
  `precision_sampling`, `precision_insert`, `precision_conversion_structure`,
  the export renders (`doc_export.rs:1271/1301`, `doc_export_layers.rs:312`,
  `precision_io.rs:123` — the only `OutputView::Seeded` callers left),
  decoders and `precision_scalar_filter`.
- **Tests and the example**: `precision_layer_producers` (new,
  HEAD-compilable): `record_layer_producers` prints `producer
  <site>/<depth> <WxH> <sha256> checkpoints=<n> reports=<sha256>` for 18
  site / depth pairs × 3 fixtures; `no_view_layer_producers` fails at HEAD
  by design; `layer_renders_equal_the_projection` passes there.
  Tightened (each fails at `63421b3` by design):
  `precision_image_writers` (a) / (b) / (c),
  `precision_retouch_writers::no_view_retouch_writers` (WHOLE and
  no-change arms 0), `precision_rect_writers::no_view_rect_writers` (a
  GROWN answer 0), `precision_constructor_seeds::no_view_converted_input`
  (0). Moved: `precision_projection_unseeded::seeded_outputs_keep_their_view`
  (deleted, now the new suite's), the `precision_constructor_seeds` and
  `precision_plates` `seeded_outputs_keep_their_view` keep only
  `add_layer_at` / the geometry half (13B-3d-3b). `phase13b_tile_sharing`
  gains `typed/producers` and `typed/producer_consumers`
  (`RZ_TILE_SHARING_ONLY=producers`, last in the full run).
- **Counts**: the residual `rg -c 'contiguous_rgba(16|32)\(' core/src` stays
  **26** (2 definitions + 24 call lines); the seeded-site count `rg -n
  'PrecisionImage::from_rgba16\(|PrecisionImage::from_rgba32_extended\(|PrecisionImage::from_rgba32\(|::from_float_samples\(|::from_rendered_float\(|OutputView::Seeded'
  core/src | grep -v '^core/src/precision_image'` goes **47 → 35** lines.

## 13B-3d-3b contract

13B-3d-3b (this run) builds the zero planes of `add_layer_at`, the sourced
structure shapes and the narrowing's masked-adjustment plane over
`TilePlane::filled` (one tile per tile shape, no view), gives
`precision_sampling`'s resample outputs a per-caller `OutputView`, and pulls
13B-3e's save fix forward: `precision_codec::encode` walks `runs_rgba16`.
Every output (u16 codes, f32 bits), refusal and its order, checkpoint and
report sequence and `.rz` / PNG / TIFF byte is HEAD `e250ba6`'s. No FFI,
header, Swift, `.rz`, PSD, RZCB, report key, plan JSON or `schema_version`
change; no new `pub` item; no tool or argument change; `OutputView` keeps
both variants.

- **`fill`** (`precision_image_fill.rs`, a NEW `#[path]` child of
  `precision_image`, so it sees the private fields; `build`'s `assembled`
  and `EXTENDED` become `pub(super)`): `pub(crate) filled_rgba16(w, h,
  value, profile)` and `filled_rgba32_extended(w, h, value, profile)`
  answer `Self::assembled(..)` over the private `filled_plane`:
  `pixel_count` first (its sentence); a plane whose view the cache would
  refuse (`w·h·size_of::<S>() > CONTIGUOUS_CACHE_BYTES`, 13B-3b-1 decision
  (c)) or that `TilePlane::filled` refuses is a contiguous `reserved_vec`
  resized with `value` (allocation failure `ALLOCATION`); else
  `Plane::Tiled(TilePlane::filled(..))`. The f32 constructor refuses
  `!extended_pixel(value)` with `EXTENDED` before allocating. Fresh
  identity, no pinned view, no seed, no control, nothing polled, no
  admission (decision D3).
- **`LayerRaster::filled(w, h, value)`** (`pub(crate)`):
  `TilePlane::filled` → `Self::tiled`, `Err` → `Self::contiguous(Arc::new(
  RgbaImage::from_pixel(..)))` (`new`'s fallback shape); no seed.
- **The four filled sites**: `add_layer_at` (`precision_insert.rs`, f32
  `filled_rgba32_extended`, u16 `filled_rgba16`; `pixel_count(width,
  height)?` stays as a statement), `precision_conversion_structure.rs`'
  `Planes::Shape` with an effective source (`filled_rgba16(w, h, [0; 4],
  ..)`; the `walk.shape` charge and `admit` unchanged) and
  `precision_conversion_narrow.rs`' masked-adjustment plane
  (`LayerRaster::filled(dims.0, dims.1, [0; 4])`). Readers: the added layer
  → the rect / runs writers, the compositor, the regional window and
  `encode` (runs); its whole readers are the 13B-3e residual
  (`float_native.rs:73` f32 save, `psd_export16.rs:363`, …). The sourced
  shape → `structure_document` / `build` / `native_fits_geometry`
  (dimensions only; dropped with the plan). The narrowed plane → the byte
  document's adjustment layer (extent through
  `doc::composite_adjustment_into`; the `.rz` / PSD writers through
  `to_contiguous` / `runs`).
- **Resample outputs**: `transform_image` / `resize_image` take `view:
  OutputView` before `job`, and the private `output16` / `output32` pick
  `from_rgba16` / `from_rgba16_unseeded` and `from_rgba32_extended` /
  `from_rgba32_extended_unseeded` (`float_document.rs:627`'s match);
  `resize_image`'s same-size `source.clone()` is unchanged. Unseeded:
  `precision_geometry.rs:151/154` (installed as the layer; `image_equal`
  walks runs), `retained_source_render.rs:373` (`render_high_depth`'s
  Sample arm → `render16`'s `rendered` installed as a layer, and
  `render8`'s quantize rows) and `develop_inspect.rs:89` (the ≤ 1 MP host
  handle; the FFI lend binds its own view). Seeded: `doc_export.rs:1316`
  (read whole by the sharpen `:1329` and `convert16` `:1097`, 13B-3e).
- **`encode`** (`precision_codec.rs`, which serves `precision_codec::save`,
  `precision_png::encode_native_png` — the typed native save through
  `precision_native.rs:508` — and the 16-bit export): `runs_rgba16()` with
  HEAD's sentence, `report(0.1, 1)`, a `reserved_vec(w·h·8)` (PNG) /
  `reserved_vec(w·h·4)` (TIFF) staging buffer and plain nested `for run in
  runs { for pixel in run { job.every(i)?; i += 1; … } }` loops with HEAD's
  per-pixel bodies, so the index sequence and every byte are HEAD's.
- **Tests and the example**: `precision_filled_outputs` (new,
  HEAD-compilable): `record_filled_outputs` prints `filled <site>/<depth>
  <WxH> <sha256> checkpoints=<n> reports=<sha256>` for 23 site / depth
  pairs × 4 fixtures (600×520, 512×512, 257×3, 1×1); `no_view_filled_planes`,
  `filled_planes_share_one_tile_per_shape`, `no_view_resample_outputs` and
  `no_view_encode` fail at HEAD by design;
  `filled_layer_renders_and_windows_equal_the_input` and
  `encoded_samples_decode_exactly` pass there. Moved (deleted):
  `precision_constructor_seeds` / `precision_plates`
  `seeded_outputs_keep_their_view`. Carried nit 2:
  `precision_image_writers` (c) pins `w·h·(8 + 16)` views after the f32-base
  rebuild. `phase13b_tile_sharing` gains `typed/filled`
  (`RZ_TILE_SHARING_ONLY=filled`, last in the full run).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **26 → 25** (2
  definitions + 23 call lines); the seeded-site count (13B-3d-3a's `rg`)
  **35 → 33** lines (−2 `add_layer_at`, −3 `precision_sampling`, +2 the
  seeded arms of `output16` / `output32`, +1 `doc_export.rs:1320`'s
  `OutputView::Seeded`).

## 13B-3e-1 contract

13B-3e-1 (this run; 13B-3e SPLIT into four runs, plan table) moves the typed
export chain, the 16-bit PSD writer and the float TIFF / float native
writers onto tile reads (runs, rows, one rect) and stops every export
render, export constructor and the style bake from seeding the view cache.
Every output byte (`.rz` / PSD / PNG / TIFF / JPEG), typed sample, refusal
sentence and its order, checkpoint count and report log is HEAD
`2c04c47`'s. No FFI, header, Swift, `.rz`, report key, plan JSON, tool or
argument change; no new `pub` item; `OutputView` keeps both variants;
admission and plans unchanged (`export_plan`, `export_layer_plan`,
`export_scratch_bytes`, `save_psd`'s `canvas_bytes`; tightening is 13B-8).

- **`ops_photographic::unsharp_mask_runs`** (`pub(crate)`, NEW):
  `unsharp_mask_samples`' body over `runs: impl Iterator<Item = &[[T;
  4]]>` and a `count`, with its refusals in its order (radius, amount /
  threshold, the bypass `None`, `count != w·h` "unsharp mask sample count
  differs from the dimensions", `neighborhood_plan`), then the same fill
  over the runs in nested loops. `unsharp_mask_samples` delegates
  (`std::iter::once(source)`, `source.len()`), so the byte export and the
  byte filter are unchanged.
- **`doc_export.rs`**: `convert16` walks `runs_rgba16` in nested `for run /
  for pixel` loops (`job.every(i)`, `i` taken at the top, HEAD's body),
  `reserved_vec(w·h)`, output `from_rgba16_unseeded`; the sharpen feeds
  `unsharp_mask_runs::<u16>(resampled.runs_rgba16()…, w·h, (w, h), …)`,
  output `from_rgba16_unseeded`; `export_encoded`'s render, `export_stages`'
  tone map (the size `if` collapses) and resize pass `OutputView::Unseeded`.
  +5 lines (1461).
- **`doc_export_layers.rs`**: the private `Extent` (HEAD `bounds_from`'s
  four extremes + `any`, the same initial values; `include`, `rect`) serves
  `bounds_from` (the byte scan, unchanged answers) and `alpha_bounds16(image:
  &PrecisionImage)` (the sole caller's signature changed), which walks
  TILE-BAND order (each 256-row band, each 256-column span, each row's
  `row_segments(y, x0, x1)` runs, `PrecisionRun::Rgba16`) and includes each
  run's first and last visible pixel; `crop16` keeps its two refusals in
  order, then ONE `read_rect_rgba16(..).ok_or("could not allocate precision
  output")` → `from_rgba16_unseeded`; `export_layer_encoded` renders
  `Unseeded`, and the trim is a `storage()` gate with HEAD's first sentence
  (the unreachable "covering the canvas" short-slice refusal is removed).
- **`psd_export_encode::composite_planes16(composite: &PrecisionImage, w,
  h)`** (sole caller `psd_export16.rs`): the dimension check first (HEAD's
  sentence), then ONE pass over `runs_rgba16` filling the four `w·h·2`
  planes (alpha raw, RGB through `matte16`).
- **`psd_export16.rs`**: the private `samples16(image)` =
  `image.runs_rgba16().map(Iterator::flatten)`; the layer records and the
  flattened record read through it after `storage()` gates carrying HEAD's
  sentences at HEAD's positions — both through the private `channels16`
  (the four channel planes, ids −1 / 0 / 1 / 2, from ONE walk of
  `samples16`, each `encode_plane`d as `plane16` did; decision D5);
  `composite_planes16(projection, …)`.
- **The bake**: `bake_style16` builds its image with
  `from_float_samples_unseeded` (the same arguments).
- **`float_io::encode_float_tiff`** walks `runs_rgba32` (HEAD's
  `job.every(i)` sequence), `samples` reserved `w·h·4`;
  **`float_native::put_raster`** walks `runs_rgba32` into the 4096-sample
  staging array, and each FULL chunk and the final partial one flush
  `job.check()?` then `append` (HEAD's chunk sizes and poll sequence);
  `length = w·h·16`.
- **`precision_io::export16`** renders `OutputView::Unseeded`.
- **`OutputView::Seeded`** keeps no caller: `#[allow(dead_code)]` and a doc
  line (removed with its arms in 13B-3e-4).
- **Tests and the example**: `precision_export_reads` (NEW,
  HEAD-compilable): `record_export_reads` prints `export <site> <WxH>
  <sha256> checkpoints=<n> reports=<sha256>` for 18 sites × 4 fixtures
  (600×520, 512×512, 257×3, 1×1) — `encoded/png16_sharpen`,
  `encoded/tiff16_p3`, `encoded/png16_half_sharpen_p3`, `encoded/png8_p3`,
  `encoded/tiff16_render_sharpen_p3` (NULL projection), `layer/png16_trimmed`,
  `layer/tiff16_p3_trimmed`, `layer/png16_canvas` (layer 1),
  `layer/empty_trimmed` (an `add_layer_at` empty top layer → refusal),
  `export16/png` / `/tiff`, `psd/flattened` / `psd/layers_baked` (layer 1
  styled `{"effects":[{"type":"stroke","size":0.5}]}`, baked), `float/tiff32`,
  `float/tiff32_render`, `float/png16`, `float/png16_half_sharpen`,
  `float/native`; `no_view_export_reads` fails at HEAD by design;
  `trimmed_layer_export_is_the_canvas_export_cropped`,
  `psd_records_hold_the_source_samples` and
  `float_exports_round_trip_exactly` pass there. Carried nit 2:
  `precision_filled_outputs`' `retained_bytes()` oracle takes its profile
  term from a separate 1×1 probe (decision D7). `phase13b_tile_sharing`
  gains `typed/exports` (`RZ_TILE_SHARING_ONLY=exports`, last in the full
  run).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **25 → 17** (2
  definitions + 15 call lines, 13 files); the seeded-site count (13B-3d-3a's
  `rg`) **33 → 24** lines (`doc_export.rs:1117/1271/1301/1320/1340`,
  `doc_export_layers.rs:135/312`, `precision_io.rs:123`,
  `precision_style_render.rs:919`).

## 13B-3e-2 contract

13B-3e-2 (this run) moves the typed live filter and the retained-source
render onto tile reads and builds their outputs unseeded, so a typed
live-filter EDIT or a retained-source render leaves no view of its input.
The one exception is the 16-bit preview, which MISSED the warm bound and
REVERTED to HEAD's read and its feeding seed (D5). Every answer, refusal
sentence and its order, checkpoint count and report log is HEAD `3f7e2e5`'s.
No FFI, header, Swift, `.rz`, report key, plan JSON, tool or argument
change and no new `pub` item. `OutputView` is untouched. Plans are
unchanged (`work_plan`, `live_filter_evaluate::plan`,
`live_filter_preview::scratch`, the cache's 128 MiB policy).

- **`live_filter_evaluate.rs`**: `straight_plane<'a, T: Unorm + 'a>(runs:
  impl Iterator<Item = &'a [[T; 4]]>, count, job)` runs nested `for run /
  for pixel` loops with a running index (`job.every(index)` first, then
  HEAD's body), `reserved_vec(count)`. `filtered` takes `source: impl
  Iterator<Item = &'a [[T; 4]]>` and moves it into whichever
  `straight_plane` branch runs (a cached prefix reads nothing); `count = w·h`
  from `dimensions`. `source8` passes `std::iter::once(image.as_raw()
  .as_chunks::<4>().0)`, so its `to_contiguous_with_job` transient and its
  checkpoints are unchanged (D4). In `source16`, the promoted byte master
  (`:396`) is `from_rgba16_unseeded`, and the coloured plane is read as
  `colored.runs_rgba16().ok_or("invalid filtered native source")?` at HEAD's
  position. The filtered output (`:423`) STAYS `from_rgba16`, Seeded, by the
  preview MISS (D5).
- **`retained_source_render.rs`**: `color16` keeps HEAD's order (the
  profile early-out, the two model refusals, `PrecisionTransform::new`).
  Then a `storage()` gate carries HEAD's "retained source requires RGBA16
  UNORM storage", followed by `reserved_vec(w·h)` and `reserved_vec(w)`. Per
  row, the row's `row_segments` runs are copied into that ONE row buffer.
  HEAD's per-pixel body then walks it with a running `i` (`job.every(i)`
  first, HEAD's refusal "source color transform produced an invalid
  component"; D6). The output is `from_rgba16_unseeded(w, h, pixels,
  to.clone(), None)`. `render8`'s 16-bit arm is
  `LayerRaster::unseeded(rendered.quantize_with_context(..)?)` (D3).
- **`live_filter_preview.rs`**: the 16-bit arm is HEAD's (`contiguous_rgba16`
  + `view.index`), with a comment naming the MISS. The spec's `row16` row
  cursor was built, measured and removed (D2, D5). The mask arm and the byte
  arm (`Colored8::sample_at`) are unchanged.
- **Refusal sentences** (all HEAD's, at HEAD's positions): "invalid filtered
  native source" (`source16`), "retained source requires RGBA16 UNORM
  storage" (`color16`'s gate; the in-loop `PrecisionRun::Rgba32` arm is
  unreachable after it), "invalid native source preview" (the preview).
- **Tests and the example**: `live_filter_tile_reads` is NEW and
  HEAD-compilable (`mod live_filter_support;`). `record_live_filter_reads`
  prints `live <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` for
  11 cases × 4 fixtures (600×520, 512×512, 257×3, 1×1): `edit/u16`,
  `edit/u16_p3`, `edit/u8_master`, `edit/u8_master_p3`,
  `edit/two_filters_reedit` (prefix and kernel cached), `edit/byte_doc_u16`,
  `preview/before_p3`, `preview/after`, `preview/after_div3`,
  `preview/byte_doc_after` and `render/rotated` (a 10° bilinear source
  transform under a control). `no_view_live_filter_reads` fails at HEAD by
  design. In the tree it asserts 0 view bytes, except that the nine cases
  the D5 revert keeps on the seeded filtered plane hold 0 or exactly that
  one plane (`w·h·8`; `edit/byte_doc_u16` holds it at 600×520, 0 at 6 MP by
  eviction); at HEAD `edit/u16` holds two. The other tests pass at
  HEAD: `tiled_edit_matches_the_double_reference` (the binary64 oracle
  under the frozen 3-code / 6-code bounds) and
  `preview_reads_the_cell_centres`. `phase13b_tile_sharing` gains
  `typed/live_filter` (`RZ_TILE_SHARING_ONLY=live_filter`, last in the full
  run).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **17 → 15**, 2
  definitions + 13 call lines in 11 files (`live_filter_evaluate.rs:409` and
  `retained_source_render.rs:252` moved; the spec's 14 / 10 assumed the
  preview read moved too). The seeded-site count (13B-3d-3a's `rg`) is
  **24 → 22** lines (`live_filter_evaluate.rs:396`,
  `retained_source_render.rs:272`; `:423` kept).

## 13B-3e-3a contract

13B-3e-3 SPLIT at its spec (D1): **13B-3e-3a** (this run) moves the six
remaining whole readers of an installed typed layer or caller image onto
tile reads and builds the scalar filter's gray image unseeded, so none binds
or seeds a view. **13B-3e-3b** (planned) owns raw develop / geometry /
inspection. Every answer, refusal sentence and its order, checkpoint count
and report log is HEAD `a2541cf`'s. No FFI, header, Swift, `.rz`, report
key, plan JSON, tool or argument change and no new `pub` / `pub(crate)`
item. Plans are unchanged (`warp_plan`, `selected_edit_plan`,
`precision_scalar_filter::plan`, `refinement_cutout_plan`,
`PlaneRead::plan`). The warp `mask()` arm (`PrecisionPlane`, 13B-4) and
`warp_native`'s seeded WARP decode (13B-3e-4) are untouched.

- **`selected_filter.rs`** (Pixelate): no `samples` bind. HEAD's block loops
  and every `job.check()` are kept; each block row is read through
  `row_segments(y, cx, cx + cw)` and each `PrecisionRun::Rgba16` run's
  samples feed HEAD's u64 sums (D3). A `Rgba32` run answers Err "selected
  edit requires u16 source" (the caller gate's sentence,
  `selected_adjustment.rs:151`; unreachable, HEAD `unwrap`ped).
- **`selected_auto_levels.rs`**: a `storage()` gate carrying "automatic tone
  requires native u16" at HEAD's position (before `bins`). The histogram
  pass walks TILE-BAND order (D2), flat `i = y·w + x` for `weight(i)`, HEAD's
  body (order-independent u64 counters). The `auto_color` pass walks
  `runs_rgba16()` row-major (its f64 sums are order-dependent) with a
  running `i`. In both passes the flat walk's per-sample `job.every(i)` is
  HOISTED to each run (one `check` per multiple of 4096 in the run's visit
  range: the same checkpoints; the fix pass, D7).
- **`precision_plane_read.rs`** (the RGBA component arm): the `storage()`
  gate ("high-depth plane read requires u16 storage") at HEAD's position;
  `out.resize(n, 0)` and a TILE-BAND walk writing `out[y·w + x] =
  p[channel]` with `job.every(visited)` first. The canvas placement after
  the match is unchanged.
- **`precision_cutout.rs`**: the `storage()` gate ("native refinement
  foreground must be RGBA16") after the matte length check, before
  admission; the nonempty scan in TILE-BAND order over the matte by flat
  index, every sample visited (no early exit), the checks hoisted per run
  and the run zipped with its matte span (the fix pass, D7).
- **`warp_native.rs`** (`encode_typed`): `p.runs_rgba16().ok_or("warp native
  requires RGBA16")?` at HEAD's position; the length word `w·h·8` from
  `dimensions()`; each run's codes written LE into its grown span (one
  `resize` per run; the fix pass, D7) — HEAD's byte stream, row-major.
- **`precision_warp.rs`** (`image`): a `storage()` gate carrying "warp is
  unavailable at 32-bit float", then ONE `transient_rgba16()?` per call (a
  contiguous plane borrowed, a tiled one copied; `|i| samples[i]`
  unchanged; D4). A failed copy answers "could not allocate precision
  output" where HEAD's materialization panicked.
- **`precision_scalar_filter.rs:53/156`**: `from_rgba16_unseeded`, same
  arguments (D5).
- **Direct payload paste** (`clipboard_paste`'s no-convert arm,
  `from_clipboard`): no code change (D6).
- **Tests and the example**: `precision_whole_readers` is NEW and
  HEAD-compilable. `record_whole_reads` prints `whole <case> <WxH> <sha256>
  checkpoints=<n> reports=<sha256>` for 15 cases × 4 fixtures (600×520,
  512×512, 257×3, 1×1): `selected/pixelate7`, `selected/pixelate7_rect`,
  `selected/auto_color_selection`, `selected/auto_tone_apply`, `plane/red`,
  `plane/alpha_canvas`, `scalar/pixelate5`, `scalar/auto_contrast`,
  `cutout/matte`, `cutout/empty`, `warp/edit`, `warp/reedit`, `warp/save`,
  `paste/read_plane`, `paste/from_clipboard_pixelate`. At 1×1 the warp field
  exceeds the contraction bound, so `warp/edit` answers its refusal and
  `warp/reedit` / `warp/save` build on the unwarped document.
  `no_view_whole_reads` fails at HEAD by design; the other four pass there
  (`plane_read_matches_the_samples`, `pixelate_matches_the_block_means` —
  opaque, within 1 code of the u64 block mean written in the test —
  `cutout_layer_is_the_foreground`). `precision_layer_producers`'
  `no_view_layer_producers` expects 0 for the warp too (was `w·h·8`, the
  source bind). `phase13b_tile_sharing` gains `typed/whole_readers`
  (`RZ_TILE_SHARING_ONLY=whole_readers`, last in the full run).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **15 → 9**, 2
  definitions + 7 call lines in 5 files: the lend `precision_image.rs:307/323`,
  `live_filter_preview.rs:164` (the 13B-3e-2 MISS), `develop_inspect.rs:136`,
  `raw_local_develop.rs:163/183`, `raw_source_geometry.rs:392`. The
  seeded-site count (13B-3d-3a's `rg`) is **22 → 20** lines.

## 13B-3e-3b contract

**13B-3e-3b** (this run) moves the raw develop's two whole reads onto tile
runs and tried the develop inspection preview on tile reads, under a warm
bound FIXED before measuring (≤ 1.05 × HEAD, the median of 3 alternated
samples at equal load). Every answer, refusal sentence and its position,
checkpoint count and report log is HEAD `58f8304`'s (core = `5ec96ed`'s).
No FFI, header, Swift, `.rz`, PSD, report key, plan JSON, tool or argument
change and no new `pub` / `pub(crate)` read API; one `pub(crate)` constant
(`job_control::EVERY_INTERVAL`). Plans are unchanged (develop's
`PARSE_SCRATCH` / `scratch` / `work`, `raw_source_geometry_plan`'s JSON and
`plan.scratch`, the preview's `scratch`).

- **`raw_local_develop.rs`** (`develop`): `images` is `Vec<&PrecisionImage>`.
  The variant loop keeps a `storage()` gate carrying "local variant requires
  genuine RGBA16 samples" at HEAD's position (after `settings.plan(mask)?` /
  `work`, BEFORE the early `Ok(None)`); the base's gate carrying "local base
  requires genuine RGBA16 samples" sits after `job.reserve(scratch)`. Pass 1
  (base → `accumulated`), pass 2 (per enabled variant) and pass 3 (quantize,
  `changed` against the base) walk `runs_rgba16()` row-major with a running
  flat `i` (`.ok_or(<the gate's sentence>)`). Pass 2 keeps HEAD's per-sample
  `job.every(i)` → `effective.at(i)?` → weight test; passes 1 and 3 hoist
  `job.every(i)` to each run through `EVERY_INTERVAL` (the fix pass, D3).
  The output stays `PrecisionImage::from_rgba16` (Seeded, D2).
- **`raw_source_geometry.rs`**: HEAD's body verbatim — the
  `contiguous_rgba16()` bind at `:391-393` (deferred, D1) and the Seeded
  output at `:466` (kept by the preview revert, D4).
- **`develop_inspect.rs`** (`develop_preview`): HEAD's body verbatim after
  the preview MISS (D4); the band walk measured is recorded in D4.
- **`job_control.rs`**: `pub(crate) const EVERY_INTERVAL: usize = 4096`;
  `JobContext::every` polls when `index & (EVERY_INTERVAL - 1) == 0`. The
  three hoisted loops of 13B-3e-3a (`selected_auto_levels.rs` ×2,
  `precision_cutout.rs`) step by it (carried nit 1).
- **Tests and the example**: `raw_develop_reads` is NEW and
  HEAD-compilable. `record_raw_develop_reads` prints `raw <case> <WxH>
  <sha256> checkpoints=<n> reports=<sha256>` for 18 cases × 4 fixtures
  (600×520, 512×512, 257×3, 1×1) — `develop/vignette`,
  `develop/vignette_reference`, `develop/local`,
  `develop/local_feather_two`, `develop/unchanged`, `develop/disabled`,
  `geometry/exif6`, `geometry/exif3`, `geometry/affine_translate`,
  `geometry/affine_bicubic`, `geometry/lens_bilinear`, `geometry/identity`,
  `preview/fit3`, `preview/fit3_clipping`, `preview/region2_clipping1`,
  `preview/detail_clipping2`, `preview/one_cell`,
  `pipeline/develop_geometry_preview` (a multi-step case sums its steps'
  checkpoints and concatenates their logs) — plus the four float refusals at
  the `float_11f/scene.tiff` fixture's 48×32 (`refuse/float_base`,
  `refuse/float_geometry`, `refuse/float_variant` (disabled, no vignette:
  refused in the loop before the early `None`),
  `refuse/float_variant_enabled`), each asserting its sentence.
  `no_view_raw_develop_reads` fails at HEAD by design (the develop holds its
  inputs' views); in the tree it pins the kept views exactly: a develop
  `Some` its output's seed, a `None` 0, a geometry answer the source's view
  plus its output's seed (the source's view alone for the identity), a
  preview the source's view, the pipeline the develop and geometry outputs'
  seeds. `preview_matches_the_cell_rule` and
  `exif_geometry_is_the_permutation` are independent oracles and pass at
  HEAD. `phase13b_tile_sharing` gains `typed/raw_develop`
  (`RZ_TILE_SHARING_ONLY=raw_develop`, last in the full run; the three
  preview lines report `ms_warm` as the median of 9 warm calls and
  `ms_warm_min`).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **9 → 7**, 2
  definitions + 5 call lines in 4 files: the lend
  `precision_image.rs:307/323`, `live_filter_preview.rs:164` (the 13B-3e-2
  MISS), `develop_inspect.rs:136` (the 13B-3e-3b MISS) and
  `raw_source_geometry.rs:392` (deferred, D1). The seeded-site count
  (13B-3d-3a's `rg`) is **20 → 20** lines (`raw_source_geometry.rs:466`
  kept by the revert, `raw_local_develop.rs:264` deferred).
- **9f probe**: `scripts/verify_phase9f.py`'s depth-16 channel-target block
  expects 9G's admitted behaviour from the source, not from a reply:
  `red` / `alpha` APPLY (`PaintTarget.paintablePlanes`, routed to
  `precisionScalarFilter`; reply `target`, `filter` `invert`, `storage`
  `u16`, `changed`, then undo restores the raw layer) and `mask` ("The
  target layer has no mask.") / `channel:missing` (`No channel named
  "missing"`) REFUSE with their text asserted.

## 13B-3e-4a contract

**13B-3e-4a** (this run; 13B-3e-4 SPLIT into 4a / 4b / 4c at its spec, D1)
stops the two host intakes and the RAW develop output seeding the view
cache, reads `raw_source_geometry`'s source through ONE transient that
replaces their seed, and makes the FFI lend pin a PRIVATE copy that never
reads or writes the view cache. Every answer, refusal sentence and its
position, checkpoint count, report log and native `.rz` byte is HEAD
`6b97c48`'s. One new FFI export, `rz_precision_image_storage_identity`
(header, `RasterPrecisionImage.storageIdentity`); no signature change; no
`.rz`, PSD, report key, plan JSON, tool or argument change; no new
`pub(crate)` item (one `pub(super)` `Plane::lent`); no README /
MCP-reference change and no owned MCP drive (no user-visible behaviour
change).

- **`ffi_precision_input.rs:76`** (`rz_precision_doc_from_rgba16`),
  **`host_pixels.rs:236`** (`HostPixelPlan::typed`, behind
  `rz_precision_doc_from_host_pixels`) and **`raw_local_develop.rs:302`**
  (the develop output, the status doc's earlier `:264`):
  `PrecisionImage::from_rgba16` → `from_rgba16_unseeded`, same arguments
  (D2).
- **`raw_source_geometry.rs`** (`raw_source_geometry` only): HEAD's
  `contiguous_rgba16()` bind at `:391-393` becomes a `storage()` gate
  carrying "RAW source geometry requires genuine RGBA16 storage; byte/float
  unsupported" at the same position (before `parse`); ONE
  `image.transient_rgba16()?` after the `plan.unchanged` return and before
  `reserved_vec(n)?`, with the exact arm's `source[...]` and the filtered
  arm's `|i| source[i]` unchanged. An identity request copies nothing; a
  failed copy answers "could not allocate precision output" where HEAD's
  materialization panicked. `raw_source_geometry_plan` / `plan.scratch`
  unchanged. The output (`:470`, HEAD's `:466`) stays `from_rgba16`
  (Seeded, 13B-3e-4c) (D3).
- **`precision_image_plane.rs`**: `Plane::lent(&self) -> Result<Arc<Vec<S>>,
  String>` (`pub(super)`) — Tiled: `Arc::new(plane.to_contiguous(None))`,
  Err `ALLOCATION`; Contiguous: `Arc::clone` of its own buffer (zero copy,
  as HEAD). No view cache, no job, no checkpoint. `Pinned`'s doc names the
  private copy.
- **`precision_image.rs`**: `pinned_rgba16` / `pinned_rgba32` (signatures
  unchanged) answer `None` for the other storage or an allocation failure
  (the FFI's NULL, the header's existing "NULL … when the view cannot be
  allocated"; HEAD reached NULL through a caught panic); once per handle:
  `self.pinned.get()`, else `Plane::lent`, `self.pinned.set(..)` (a racing
  loser drops its copy), answer `self.pinned.get()`. A clone starts empty;
  a pin is reported as retained `precision_views` (unchanged). The module
  doc's THE FFI LEND paragraph and the residual sentence ("5 after
  13B-3e-3b; 2 after 13B-3e-4a") follow (D4).
- **The storage identity** (amendment 1, D6):
  `rz_precision_image_storage_identity(image) -> u64` in `ffi_precision.rs`
  (after `rz_precision_image_pixels_rgba16`), declared in
  `rasterize_core.h` and wrapped as `RasterPrecisionImage.storageIdentity`
  (`RasterCore.swift`, the only Swift change), answers
  `PrecisionImage::plane_key()` widened to `u64` (0 for NULL) — no
  allocation, view, pin, job or checkpoint.
  `precision_image_tiles::the_storage_identity_names_the_shared_plane`
  pins it (a clone, `layer_source`, a profile assignment and an integer
  move keep it; a second construction, a one-pixel edit and a fractional
  move change it; a lend and an eviction leave it). The 9h Move probe
  (`scripts/native-paint-probe/geometrygestures.swift`) compares
  `geometryStorage` (the identity; throws on 0) where it compared the lend
  pointer (`geometryPointer`), with its four failure messages unchanged
  and the result key `"pointer_equal"` → `"storage_equal"` (A1 / A2).
- **Comments / docs only**: `ffi_precision.rs` / `ffi_float.rs` (the lend
  pins a private copy; an allocation failure answers NULL),
  `contiguous_views.rs` (`record_contiguous_views` no longer cites a pinned
  FFI view), `precision_render.rs` (`OutputView::Seeded` removed in
  13B-3e-4b).
- **Tests and the example**: `host_intake_unseeded` is NEW and
  HEAD-compilable (a `#[global_allocator]` wrapping `System` tracks live
  and peak bytes, `alloc_zeroed` / `realloc` included; every test holds
  `serial()`). `record_host_intake` prints `intake <case> <WxH> <sha256>
  checkpoints=<n> reports=<sha256>` for 13 cases × 4 fixtures (600×520,
  512×512, 257×3, 1×1): `intake/from_rgba16`, `intake/host_rgba16_le`
  (`row_bytes = w·8 + 16`), `intake/host_argb16_be_orient6`,
  `develop/intake_vignette`, `geometry/intake_exif6`,
  `geometry/intake_lens_bilinear`, `geometry/intake_identity`,
  `pipeline/intake_develop_lens` (the app order), `pipeline/intake_preview`
  (whole, divisor 3, clipping 3), `save/intake_native`,
  `save/developed_native` (`encode_native`, `LIDS` ids zeroed),
  `lend/intake_u16` (the lent bytes) and `lend/float` (a
  `from_rgba32_extended` image lent after `evict()`, sample bits).
  `no_view_host_intake` fails at HEAD by design; in the tree, after
  `evict()` and with each answer held, `contiguous_view_bytes()` is 0 for
  the intakes, the develop, the identity geometry, the saves and the lends
  (with `precision_views` = `w·h·bpp`), the output's seed for a geometry
  answer and for `pipeline/intake_develop_lens`, and `w·h·8` (the first
  preview's view) for `pipeline/intake_preview`.
  `intake_samples_match_the_host_buffer` is an independent oracle (the
  big-endian decode and the EXIF-6 map written in the test).
  `peak_host_intake` prints `peak <case> <WxH> peak=<bytes> held=<bytes>`
  at 600×520 for nine cases. `raw_develop_reads::no_view_raw_develop_reads`
  expects develop `Some` 0, geometry `Some` `dw·dh·8`, geometry `None` 0,
  preview `plane`, pipeline `plane`; `precision_image_tiles::the_ffi_lend_is_pinned_by_the_handle`
  pins the private lend (the seed a separate cache allocation until
  `evict()`, the clone's own copy, 0 views). `phase13b_tile_sharing` gains
  `typed/raw_intake` (`RZ_TILE_SHARING_ONLY=raw_intake`, last in the full
  run).
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src` **7 → 4** in 3
  files (2 definitions + `live_filter_preview.rs:164`,
  `develop_inspect.rs:136`); the seeded-site `rg` of the 13B-3d-3a contract
  **20 → 17** lines.

## 13B-3e-4b contract

**13B-3e-4b** (this run; 13B-3e-4 part 2, 13B-3e-4a D1) builds every
reader-side typed decoder's plane without seeding the view cache, fills a
bitwise-uniform decoded plane (the reopened empty layer), unseeds the
narrowing's three byte planes and removes `OutputView::Seeded` with its
arms. Every answer, refusal sentence and its position, checkpoint count,
report log and `.rz` / PSD / RZCB / PNG / TIFF byte is HEAD `277c88a`'s. No
FFI, header, Swift, format, report key, plan JSON, tool or argument change;
no new `pub` item (two `pub(crate)` constructors, one `pub(super)` plane
helper); no README / MCP-reference change and no owned MCP drive (no
app-visible behaviour change).

- **`precision_image_build.rs`**: the private `Tiling::Decoded` and
  `tile_decoded` — `tile_unseeded`, except that a contiguous plane
  `uniform_tiles` answers `Some` for becomes `Plane::Tiled(that)` and the
  adopted `Vec` is dropped; `pub(crate)` `from_rgba16_decoded` /
  `from_rgba32_extended_decoded` run exactly `from_rgba16_unseeded` /
  `from_rgba32_extended_unseeded`'s statements (length check, `admit`,
  `reserve`, the f32 `EXTENDED` loop, `report(1.0, 5)`) with
  `Tiling::Decoded` as the tiling step: a fresh identity, never a seed,
  nothing new polled. `from_float_samples` and `from_rendered_float` (the
  seeded render constructors, no caller once the arms go) are DELETED;
  `from_rendered_float`'s `extended` doc moved onto
  `from_rendered_float_unseeded` (D1, D4).
- **`precision_image_plane.rs`**: `uniform_tiles(width, height, samples)`
  (`pub(super)`, beside `tiles_of`) answers
  `Some(TilePlane::filled(width, height, samples[0]))` iff `samples` is
  non-empty, `w·h·size_of::<S>() ≤ CONTIGUOUS_CACHE_BYTES` (`tiles_of`'s
  bound), every sample is `Sample::same_bits` to `samples[0]` (the scan
  stops at the first mismatch; f32 `-0.0` among `0.0` is NOT uniform) and
  `filled` is `Ok`; it allocates nothing else and polls nothing (D2).
- **The ten decoder lines** — `PrecisionImage::from_rgba16` →
  `from_rgba16_decoded`, same arguments: `precision_png.rs:153`
  (`decode_native_png`: typed `.rz` layers and every u16 source-asset
  master), `precision_codec.rs:495` (`decode_with_context`, the PNG16 /
  TIFF16 open), `psd_import16.rs:281` (`raster_layer`, the pixel-less
  record's canvas of zeros included) and `:427` (the composite),
  `warp_native.rs:534` (`attach_typed`, the WARP original; the
  `same_samples` dedup at `:560` still shares), `precision_clipboard_codec.rs:313`
  (`from_rgba8`) and `:545` (the u16 RZCB decode); `from_rgba32_extended`
  → `from_rgba32_extended_decoded`: `float_io.rs:158` (the float TIFF open;
  after `cargo fmt` the call wraps to `:159`), `float_native.rs:142`
  (`decode_raster`, the f32 `.rz` raster) and
  `precision_clipboard_codec.rs:568` (the f32 RZCB decode).
- **The narrowing** (`precision_conversion_narrow.rs`): `:127`
  (`unit_pixels`, every 1×1 placeholder), `:233` (the base raster) and
  `:400` (the WARP original's source pixels) — `LayerRaster::new` →
  `LayerRaster::unseeded`; `layer_raster.rs`'s `unseeded` caller list names
  them (D3).
- **`OutputView::Seeded` REMOVED** (`precision_render.rs`, with its
  `#[allow(dead_code)]`) and its five arms: `precision_render.rs:789/805`,
  `float_document.rs:627`, `precision_sampling.rs:64/78`; every `match`
  keeps its `Unseeded` arm and every signature is unchanged. The enum doc
  says it has one variant since 13B-3e-4b (the type and its `view`
  parameters → 13B-3e-4c, Open items) (D4).
- **Docs**: `precision_image.rs`'s WRITERS paragraph (the decoded
  constructors; no render or decoder seeds) and the `Clone` doc (the
  private lend, carried nit 2); `core/CLAUDE.md` line 81 (`OutputView::Seeded`
  removed in 13B-3e-4b, carried nit 3; the decoders build through the
  decoded constructors).
- **Tests and the example**: `precision_decoders_unseeded` is NEW and
  HEAD-compilable (the `host_intake_unseeded` tracking allocator; every
  test holds `serial()`). `record_decoders` prints `decode <case> <WxH>
  <sha256> checkpoints=<n> reports=<sha256>` for 18 cases × 4 fixtures
  (600×520, 512×512, 257×3, 1×1), every hashed `.rz` stream with its
  trailing `8·layer_count` `LIDS` bytes zeroed: `rz16/reopen` (a pattern
  layer, a Pixels-target WARP where the fixture admits one, an
  `add_layer_at` layer; every layer's samples + the resave), `rz16/master`
  (a layer placed from a u16 retained source; the master's samples + the
  resave), `rzf/reopen`, `png16/open`, `tiff16/open` (written by
  `precision_codec::save`), `psd16/layered` (`psd16_fixture` with a
  pixel-less record) / `psd16/composite`, `tiff32/open`, `rzcb16/decode` /
  `rzcbf/decode` (streams written in the test), `rgba8/payload`,
  `rzcb16/paste`, `narrow/u16_rgba8` (13B-3d-3b's masked-adjustment
  document + WARP) / `narrow/f32_rgba8`, `render/u16` / `render/f32` (the
  reopened document's projection), `pipeline/rz16_master_preview` and
  `pipeline/png16_preview` (decode → `rz_precision_image_develop_preview`
  whole, divisor 4, clipping 0). `no_view_decoders` (fails at HEAD by
  design) prints `views <case> 600x520 held=<bytes> expected=<bytes>` for
  every case, then asserts 0 for every decode, narrow and render case and
  `w·h·8` for the two pipelines. `reopened_uniform_planes_are_filled`
  (fails at HEAD) asserts the `.rz` u16 and f32 `add_layer_at` layers, the
  PSD pixel-less record, a uniform nonzero u16 `.rz` layer and a uniform
  RZCB payload retain exactly a fresh `add_layer_at` plane's
  `retained_bytes` (built on the same document / profile), and that a plane
  differing only in its LAST sample (`.rz`, RZCB) and an f32 plane with one
  `-0.0` among `0.0` (`.rz`, RZCB) stay dense (`precision_pixels` =
  `w·h·bpp`; not at 1×1). `decoded_samples_match_their_source` and
  `resave_is_byte_identical` pass at HEAD. `peak_decoders` prints `peak
  <case> 600x520 peak=<bytes> held=<bytes>` for all 18 cases.
  `precision_filled_outputs::no_view_filled_planes` now asserts 0 for the
  narrowing (was the base's `w·h·4`); `precision_ffi`'s null sweep gains
  `rz_precision_image_storage_identity(NULL) == 0` (carried nit 1).
  `phase13b_tile_sharing` gains `typed/decoders`
  (`RZ_TILE_SHARING_ONLY=decoders`, last in the full run; 11 lines) and
  the `narrow_masked_adjustment` text now expects 0 views.
- **Counts**: the seeded-site `rg` of the 13B-3d-3a contract **17 → 2**
  lines (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:470`);
  `rg -c 'contiguous_rgba(16|32)\(' core/src` still **4** in 3 files.

## 13B-3e-4c-1 contract

**13B-3e-4c-1** (this run; 13B-3e-4c part 1, D1) moves the two
interactive previews onto ONE narrow crate-private view accessor, deletes
the public whole-plane views and migrates every test / example reader, with
every output byte-identical to HEAD `9a30000`. No FFI, header, Swift,
`.rz` / PSD, report key, plan JSON, tool or argument change; no README /
MCP-reference change and no owned MCP drive (no app-visible behaviour
change).

- **`precision_image.rs`**: `contiguous_rgba16` / `contiguous_rgba32`
  DELETED; `pub(crate) fn preview_view_rgba16(&self) ->
  Option<Arc<Vec<[u16; 4]>>>` added in their place, its body HEAD's
  `contiguous_rgba16` body verbatim (tiled U16: `contiguous_views::view`
  of `PlaneKey::Rgba16`, a miss materializing and caching the view,
  panicking "could not allocate precision output"; contiguous U16: the
  plane's own `Arc`; F32: `None`). Its doc names it the ONE whole-plane
  view read left — the kept exception, owner 13A-latency — its only callers
  `live_filter_preview::preview`'s 16-bit arm (`live_filter_preview.rs:164`)
  and `PrecisionImage::develop_preview` (`develop_inspect.rs:137`), each
  binding it ONCE per call, and its feeding seeds
  `live_filter_evaluate.rs:429` and `raw_source_geometry.rs:471`. The
  module doc's REPRESENTATION Contiguous bullet (no f32 whole-view reader
  since 4c-1; the tiling owner 13B-3e-4c-2), ACCESSOR CONTRACT (no public
  whole-plane view; the residual "0 after 13B-3e-4c-1, the accessor
  removed") and WRITERS tail (the seeded constructors stay for the two kept
  preview seeds and test-built images; carried nit 4, rewrapped to ≤ 80
  columns) are rewritten.
- **`contiguous_views.rs`**: `View::into_rgba32` DELETED (its only caller
  went); `View::Rgba32` / `PlaneKey::Rgba32` stay for `tile()`'s f32 seed
  (13B-3e-4c-2).
- **The two previews**: `.contiguous_rgba16()` → `.preview_view_rgba16()`
  at `live_filter_preview.rs:164` (the `:159-162` comment rewritten in
  place, four lines naming the exception and its owner) and at
  `develop_inspect.rs:137` (one comment line added above `let pixels =
  self`, so the read moved from HEAD's `:136`), the same `.ok_or(..)?`
  refusal at the same position. Runtime code is otherwise HEAD's.
- **The two seeds** keep `PrecisionImage::from_rgba16`: comment only —
  `live_filter_evaluate.rs:428` rewritten in place (the seed stays at
  `:429`) and one line inserted above `raw_source_geometry.rs`'s seed, which
  moves from HEAD's `:470` to `:471`.
- **Carried nits**: 1 and 2 in `core/CLAUDE.md` (lines 81 and 15), 3 in
  `float_document.rs:607` (the doc names `from_float_samples_unseeded`), 4
  in `precision_image.rs`; 5 re-owned to 13B-3e-4c-2 (Open items).
- **Tests and the example** — 81 test files and
  `core/examples/phase7_precision.rs`, 421 calls on 406 lines plus 4
  fn-path uses, migrated in this order: (a) `x.contiguous_rgba16()` →
  `x.to_rgba16().ok()` (and the f32 twin; a now-invalid leading `*`
  dropped; `clippy::ok_expect` made four `.ok().expect(..)` chains in
  `export_production.rs` / `float_exact_geometry.rs` a plain `.expect(..)`);
  (b) the 4 fn-path uses (`precision_canvas_rotation.rs:182/184`,
  `precision_document.rs:143/144`) → `|i| i.to_rgba16().ok()`; (c) the
  per-sample reads → `rgba16_at` / `sample_at` at `(i % w, i / w)`:
  `precision_groups.rs:76-79`, `precision_prototype.rs` `decoded`, the
  example's `decoded`, `precision_masks.rs:230` (inside the x/y loop) and
  `precision_retouch.rs:233` (inside a per-channel loop); (d) the
  view-ADDRESS oracles → `rz_precision_image_storage_identity(image) as
  usize`: `precision_stamp_flatten.rs` `pixel_address`,
  `precision_resolution.rs` `addresses`, `precision_canvas_size.rs`
  `raster_addresses`, `precision_structure_ops.rs` `pixel_address`,
  `precision_insert.rs` `pixel_address` (their "a raster record has
  samples" storage-match panic arms went with the match); (e) the subject
  tests rewritten, below. `develop_inspection.rs` (`:185`, `:210`,
  `:213-214`) took rule (a) only. No test is deleted.
- **Deleted or replaced assertions** (rule (f)):
  `precision_image_accessor::views_bind_once_and_answer_none_for_the_other_storage`
  — the two `Arc::ptr_eq(&first, &second)` (u16, f32) are gone; both
  copies equal the oracle, and the two `is_none()` checks are now the
  `to_rgba32` / `to_rgba16` Err sentences.
  `owned_copies_equal_the_samples_in_a_new_allocation_and_refuse_the_other_storage`
  — the two copy-vs-view `assert_ne!(copy.as_ptr(), view.as_ptr())` are
  gone. `precision_image_tiles::round_trips_are_exact_on_both_storages_around_the_tile_edge`
  — the "view" asserts are `runs_rgba16/32().flatten()` equality.
  `a_construction_seeds_the_cache_with_the_adopted_buffer` keeps both seed
  deltas; "the first view is the seed", "a fresh materialization", the
  post-eviction `*fresh == expected`, "two binds share one view" and the
  f32 seed-pointer assert are gone from it and pinned instead by
  `preview_view_exception` (a) and (b) through the one remaining reader.
  `the_report_counts_tiles_the_index_and_the_seed_once` — "a bind retains
  nothing" is "a copy retains nothing". The ignored
  `fallback_above_the_cache_bound_stays_contiguous` — the two view-pointer
  asserts are the storage identity == the adopted `pointer as u64` and the
  `rz_precision_image_pixels_rgba32` lend == `pointer`.
  `precision_image_writers` (b) — the view pointers are the storage
  identity (the no-change rebuild's equals the base's and is not the
  adopted `buffer`); (c) at `:485-489` — "the first whole read materializes
  exactly one view" is "an owned copy binds no view" (views stay 0).
- **`preview_view_exception.rs`** (NEW, black-box, compiles and passes at
  HEAD `9a30000`): `host_intake_unseeded`'s tracking `#[global_allocator]`
  (live + peak, `alloc_zeroed` / `realloc` included), `serial()`, `evict()`
  through `rz_resources_evict_caches`; a 600×520 patterned u16 fixture;
  every call `rz_precision_image_develop_preview(img, 0, 0, w, h, 4, 0, …)`
  checked against an oracle written in the test (per cell `x1 = min(x0 + 4,
  w)`, the centre `(x0 + (x1 − x0)/2, y0 + (y1 − y0)/2)`,
  `(f64::from(v)/257.0).round() as u8`). (a)
  `a_seeded_plane_serves_the_preview_from_its_seed`: after `evict()` a
  `from_rgba16` image holds views `w·h·8`; the preview's peak above its
  baseline is < `w·h·8` and the views are unchanged. (b)
  `an_unseeded_or_evicted_plane_materializes_one_view_on_its_first_preview`,
  for a `from_rgba16_unseeded` image (views 0 before) and a seeded image
  after `evict()`: the first preview leaves views == `w·h·8` with peak ≥
  `w·h·8`; a second preview has peak < `w·h·8`, leaves the views unchanged
  and gives equal bytes.
- **Counts**: `rg -c 'contiguous_rgba(16|32)\(' core/src core/tests
  core/examples` **4 in 3 files + 406 lines → 0**; `rg -n
  'preview_view_rgba16\(' core/src` 3 lines (the definition,
  `live_filter_preview.rs`, `develop_inspect.rs`); the seeded-site `rg`
  stays **2** lines (`live_filter_evaluate.rs:429`,
  `raw_source_geometry.rs:471`; HEAD `:429`, `:470`).

## 13B-3e-4c-2a contract

**13B-3e-4c-2a** (this run; 13B-3e-4c-2 part 1, D1) deletes the
single-variant `OutputView` with its nine `view` parameters and stops every
f32 constructor seeding the view cache, with every output byte-identical to
HEAD `31981c3`. No FFI, header, Swift, `.rz` / PSD / RZCB / TIFF, report
key, plan JSON, tool or argument change; no README / MCP-reference change
and no owned MCP drive (no app-visible behaviour change).

- **`OutputView` DELETED** (`precision_render.rs`, the enum and its doc).
  The parameter is gone from `PrecisionScene::render_with_context` (the
  output `match (bool, view)` is an `if` on storage alone, the same two
  constructors `from_rendered_float_unseeded` /
  `from_float_samples_unseeded`), `PrecisionDocument::render_with_context` /
  `render_cut_with_context` (`render` and `render_seeded_with_context` drop
  their argument; the latter's doc says "unseeded"),
  `PrecisionDocument::tone_mapped_u16` (calls `from_float_samples_unseeded`
  directly), `analysis_source` (`precision_scoped_source.rs`),
  `transform_image` / `resize_image` (`precision_sampling.rs`). Every other
  parameter and its order is unchanged, `job` last. The private
  `output16` / `output32` are DELETED: the f32 transform output is
  `PrecisionImage::from_rgba32_extended(.., None)` (`:88`), the two u16
  outputs `from_rgba16_unseeded(.., None)` (`:109`, `:203`). Call sites
  lose the argument and the import only: `precision_geometry`,
  `develop_inspect`, `retained_source_render`, `doc_export`,
  `doc_export_layers`, `precision_conversion`, `precision_conversion_narrow`,
  `precision_conversion_structure` (the import keeps `MAX_RECORDS`),
  `precision_merge`, `precision_selection`, `precision_color_policy`,
  `precision_io`, `precision_retouch_source`, `precision_clipboard`,
  `float_document`.
- **No f32 constructor seeds** (`precision_image_build.rs`): `tile()`
  seeds only a contiguous u16 plane (any other plane →
  `tile_unseeded()`), so `Tiling::Seeded` means "seed a u16 buffer";
  `from_rgba32` tiles through `tile_unseeded()` and `from_rgba32_extended`
  through `Tiling::Unseeded`. Their signatures, statements, refusals and
  checkpoints are unchanged; their callers are `core/tests` and
  `core/examples/phase7_precision.rs` only. `from_rgba32_extended_unseeded`
  is DELETED (identical to `from_rgba32_extended` once that one seeds
  nothing, D2) and its two callers — the f32 Copy / Cut payload
  (`precision_clipboard.rs:261`) and the transform output above — call
  `from_rgba32_extended`. The two docs naming the seeded output view say
  "went in 13B-3e-4b". The file is 594 lines (carried nit 5).
- **`contiguous_views.rs`**: `PlaneKey::Rgba32`, `View::Rgba32` and their
  arms in `addr` / `alive` / `same` / `bytes` / `record` / `pin` DELETED;
  the module doc says a typed view is u16 only and the bounds are shared by
  "both" view types. `seed` / `view` / eviction are untouched.
- **Docs**: `precision_image.rs`'s module doc (the Tiled bullet: only the
  seeded u16 `from_rgba16` seeds; the Contiguous bullet: the tiling owner
  13B-3e-4c-2b; WRITERS: no output view, no f32 constructor seeds);
  `core/CLAUDE.md` line 81 (every output-view / `view`-parameter clause in
  the past tense with "since 13B-3e-4c-2a … always unseeded"; the payload's
  `Window::f32` → `from_rgba32_extended`; no f32 constructor seeds) and the
  `contiguous_views.rs` entry (`PlaneKey::{Rgba8, Rgba16}`,
  `View::{Rgba8, Rgba16}`); three test comments reworded
  (`precision_plates.rs:7`, `precision_filled_outputs.rs:5`,
  `precision_image_reads.rs:535`). Carried nit 1:
  `float_document.rs`'s `tone_mapped_u16` doc rewrapped to ≤ 80 columns.
  Carried nit 2: `precision_image_accessor`'s
  `views_bind_once_and_answer_none_for_the_other_storage` →
  `repeated_owned_copies_equal_the_samples_and_refuse_the_other_storage`,
  `binding_a_view_does_not_change_retained_bytes` →
  `an_owned_copy_does_not_change_retained_bytes` (its local `view` →
  `copy`).
- **Changed expectations (the f32 seed, nothing else)**:
  `precision_image_tiles::a_construction_seeds_the_cache_with_the_adopted_buffer`
  — the f32 construction's view delta is 0 (the u16 one stays `w·h·8`);
  `the_report_counts_tiles_the_index_and_the_seed_once` — per storage, the
  u16 image 1 `contiguous_views` allocation / `w·h·8` bytes, the f32 image
  0 / 0 (a clone likewise); `precision_image_writers::c_a_rebuild_over_other_dimensions_or_storage_tiles_fresh`
  — `W·H·8` (the u16 base's seed only; the f32 base seeds nothing since
  13B-3e-4c-2a). No other assertion in `make test` encoded an f32 seed.
- **`precision_f32_unseeded.rs`** (NEW, black-box, compiles at HEAD
  `31981c3`; every test holds `serial()`; `host_intake_unseeded`'s
  tracking `#[global_allocator]`; `precision_constructor_seeds`' `recorded`
  / `Line` pattern): `record_f32_constructors` prints `f32 <case> <WxH>
  <sha256> checkpoints=<n> reports=<sha256>` over 600×520, 512×512, 257×3
  and 1×1 for `ctor/from_rgba32`, `ctor/from_rgba32_extended`,
  `ctor/from_rgba32_refused` (NaN last sample),
  `ctor/from_rgba32_extended_refused` (70000.0 last sample), `rebuilt/f32`
  (last sample changed; bits + `shared_tile_count`), `rect/f32`
  (`with_rect_rgba32` of the last ≤ 3×2 corner), `convert/f32_to_u16` and
  `lend/f32` (the `rz_precision_image_pixels_rgba32` bits);
  `f32_samples_match_their_source` (`to_rgba32`, `runs_rgba32` and the lend
  equal the input bits; the two refusal sentences); `no_view_f32_constructors`
  (after `evict()`: `ctor/from_rgba32`, `ctor/from_rgba32_extended`,
  `lend/seeded_f32` 0 view bytes, control `ctor/from_rgba16` `w·h·8`; fails
  at HEAD by design, 4992000 each); `peak_f32_constructors` (`peak` /
  `held` above a per-case baseline for the three f32 cases, `rebuilt/f32`
  and the u16 control).
- **Counts**: `rg -n 'OutputView' core` 71 lines in 25 files → none;
  `rg -n 'View::Rgba32|PlaneKey::Rgba32|Rgba32\(Weak|from_rgba32_extended_unseeded|fn output(16|32)' core`
  → none; the seeded-site `rg` is now `rg -n 'PrecisionImage::from_rgba16\('
  core/src | grep -v '^core/src/precision_image'` (D3): 2 lines
  (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:471`);
  `rg -n 'preview_view_rgba16\(' core/src` 3 lines, as HEAD. Kept
  untouched: the two kept preview seeds, `preview_view_rgba16`,
  `layer_raster.rs`'s byte `View::Rgba8` / `PlaneKey::Rgba8`, and the
  above-bound f32 contiguous fallback (`precision_image_plane.rs:339-363`,
  `precision_image_fill.rs:56`), 13B-3e-4c-2b's.

## 13B-3e-4c-2b contract

**13B-3e-4c-2b** (this run; 13B-3e-4c-2 part 2) closes as a recorded
residual: no tiling code lands (D1). The diff vs HEAD `d1f7601` is
comments and one test name only:

- `precision_image.rs`'s module doc: the Contiguous bullet says no float
  document retains an above-bound raster (its native plan refuses one,
  13B-3e-4c-2b's recorded residual), so tiling that plane is 13B-8's, if
  its tighter plans ever admit one; carried nit 2 (`develop_inspect.rs:137`
  → `:135` at `:30` and `:284`). The file is 620 → 622 lines.
- `core/CLAUDE.md:81`: carried nit 2 only (the Accessor rule's
  `develop_inspect.rs:137` → `:135`; the historical `develop_inspect.rs:136`
  in the residual list stays).
- Carried nit 1: `precision_filled_outputs.rs`'s header (lines 5–9) says
  the resample outputs are always unseeded (no output view since
  13B-3e-4c-2a).
- Carried nit 3: `precision_image_tiles::the_report_counts_tiles_the_index_and_the_seed_once`
  → `the_report_counts_tiles_the_index_and_a_u16_seed_once` (the f32 half
  expects no seed since 13B-3e-4c-2a).

No FFI, header, Swift, `.rz`, PSD, RZCB, TIFF, report, plan-JSON or tool
change. No README or MCP change and no owned drive.

The kept rule is HEAD's: `tiles_of` / `uniform_tiles` refuse above the
bound (`precision_image_plane.rs:339-363`) and `filled_plane` falls back
contiguous (`precision_image_fill.rs:56`). Its readers are HEAD's:
`transient_rgba32` borrows such a plane, `Plane::lent` lends its own `Arc`,
and a changed `with_rect` copies the whole buffer once (the three ignored
tests `the_contiguous_fallback_reads_in_place`,
`fallback_above_the_cache_bound_stays_contiguous` and
`j_contiguous_fallback_rect_write_and_read` guard this).

## Decisions

- **TILE_EDGE 256.** 256 KiB RGBA8 / 512 KiB RGBA16 / 1 MiB RGBA32F per tile;
  384 tiles at 24 MP (768 at 48 MP); a 300×300 edit touches ≤ 4 tiles; the
  index is 8 B per tile (3 KiB at 24 MP).
- **Clipped edges**, not padded: exact contiguous round trip and no padding in
  the accounting; `filled` therefore needs up to four tile shapes.
- **Per-tile BITWISE latch** via `Sample::same_bits`: the store must never
  judge a signed-zero or NaN write "unchanged" — `-0.0` over `0.0` is
  stored-state movement (the `geometry_noops.rs:206-219` rule) and a NaN
  pattern over itself is not.
- **Caller-supplied category** in `record_resources`, so 13B-2 keeps layer
  tiles under `layer_pixels` and the report's category keys stay unchanged;
  the index records as `tile_structure`.
- **Core-only run**: no consumer, so every pixel output, `.rz` byte stream and
  report JSON is unchanged (`schema_version` stays 1).
- **The 13C seam**: `Tile<S>` is opaque behind `samples()`, so a spilled or
  compressed tile variant lands without changing `TilePlane`.
- **Structural sharing only**: tiles are shared by `Arc` identity between
  states derived from each other; content-addressed / interned tiles across
  documents are out of scope.
- **Test corpus include**: `core/tests/tile_store.rs` reaches the phase-4
  corpus through `#[path = "common/phase4_corpus.rs"]` (as `tests/resources.rs`
  does) because `tests/common/mod.rs` does not export it; `probe` comes from
  `mod common;`.

### 13B-2a decisions

- **Name `LayerRaster`**, because `psd_layer_pixels::LayerPixels` exists.
- **NO `Deref<Target = RgbaImage>`**, because a permanent one pins a contiguous
  buffer and forbids the 2b flip.
- **NO `PartialEq`**, because bytes-equality on tiles is a whole-plane walk and
  identity-equality would read as bytes.
- **`contiguous_image` BY VALUE**, because 2b returns a cache-produced `Arc`
  and must not re-touch sites.
- **Bind-once**, because in 2b every call is a cache lookup.
- **`rendered` / `source_pixels` / `rendered_pixels` are `LayerRaster`**,
  because `doc_warp.rs:48`, `retained_source.rs:285` and the `doc_warp.rs:404`
  translation share/compare the layer's allocation and 2b's tiles must keep
  that identity.
- **BOTH identity caches** (`style_cache`, `doc_group_query`'s content-box
  memo) key on `LayerRasterKey`, so 2b's key is the tile plane's identity,
  never a materialized `Arc`.
- **`to_contiguous` takes no job control in 2a**: no checkpoints are added, so
  there is nothing to declare or measure; 2b adds `control` when the copy
  becomes a tile walk and rewrites those sites as writers anyway.
- **The carried nit** — `read_rect` keeps `Option`, because no 2a reader
  exists to distinguish "leaves the plane" from "cannot allocate"; 2b decides
  with its first caller.
- **The regional tree binds once per resolved node**: `ResolvedNode::pixel`
  is a per-sample body, so its raster is bound into a new
  `ResolvedNode.pixels` field when the node is resolved
  (`doc_regional_tree.rs` and `resolve_materialized` in
  `doc_regional_style.rs`), the mask field's pattern — one bind per node per
  render, never one per sample.

### 13B-2b decisions

- **The view bound is one projection's working set (1 GiB, 256 entries).**
  The compositor (`doc.rs:346`, `style_render.rs:112`,
  `style_composite.rs:756`, `doc_cutout_render.rs:108`) read every byte
  layer's view per projection until 13B-2c-1 (which reads rows straight from
  the tiles, so a projection leaves no view behind); the bound was sized to
  hold a 3-layer 48 MP or 10-layer 24 MP byte document. A working set above
  it thrashes LRU
  (each projection re-materializes every layer); 13B-6's registry absorbs the
  bound and makes it working-set aware.
- **The host trim excludes the views** (`cache_bytes − contiguous_view_bytes`
  against 128 MiB), because `ImageDocument.swift:983` trims after every
  projection and would otherwise evict exactly what the next projection
  re-reads. The views stay `cache: true`, so `EditingState`'s retained figure
  still subtracts them.
- **The cache keys on the plane INDEX, never on the identity token**, so the
  `weak_count` oracle (0 fresh, 1 while a style cache holds the key) is
  unaffected, and a `reidentified` / no-change `rebuilt` raster keeps hitting
  its plane's view.
- **One entry per plane**: insert removes any live `Weak::ptr_eq` entry and a
  miss re-checks after re-locking, so a no-change `rebuilt` seed and
  concurrent materializations never duplicate a view (tested: equal-image
  `with_layer_pixels` count 1; 8 threads, count 1, all `Arc::ptr_eq`).
- **`shared` adopts as `Contiguous`**, so a placed source aliases its asset
  (the report counts asset and layer once, 2a's figure) until the first
  write, which tiles it through `rebuilt` / `with_rect`'s contiguous arms.
  The `.rz` reader's `shared` is only the 0×0 placeholder (replaced by `new`),
  so opened documents still tile.
- **An allocation failure inside a materialization panics** with "could not
  allocate layer raster storage": `contiguous_image` answers an `Arc` by value
  (no `Result` at 44 sites); `catch_unwind` reports it as `internal error: …`
  where a plain `Vec` OOM would abort.
- **The paint rect is the any-nonzero-byte box**: `blend::paint_pixel`
  returns before any arithmetic only for an all-zero Over sample and any
  alpha-0 Erase sample, so the box is a superset of both change sets and the
  output is byte-identical.
- **Deviation (implementation): the report records only the views of the
  supplied documents' planes.** Recording the whole process-wide cache in
  every report made two consecutive reports over the same handles differ when
  another thread created or dropped rasters in between
  (`linked_source_ops::mixed_inventory_deduplicates_the_genuine_original_across_handle_families`
  failed 2 of 3 parallel runs, passed serially). `record_contiguous_views`
  therefore takes the documents, walks them into a scratch inventory and
  records a view only when its plane's index allocation (what
  `TilePlane::record_resources` records as `tile_structure`) was walked —
  the scoping every per-document cache already has. Eviction stays
  process-wide.

### 13B-2c-1 decisions

- **Regional per-sample reads, not a window `read_rect`.** Every driver hands
  `resolve` the WHOLE CANVAS as `clip` (`doc_regional.rs:153`, `:201`;
  `doc_regional_plate.rs:94`), staged guides sample units over `R_0 ⊇` the
  window, and the plate samples the full canvas — so no per-node window
  exists at resolve time, and a windowed copy would either silently answer
  `None` inside `R_0` or copy the canvas. `sample_at` is allocation-free and
  exact under every driver. The 13B-2a bind-once rule for the regional tree
  (`ResolvedNode.pixels`) is RETIRED: nothing is bound.
- **Rows as runs, not a row copy.** The compositor iterates the tiles' own
  slices (`row_segments`), so a tiled layer is read in place; the per-pixel
  bodies take `(run.as_flattened(), i·4)` for their existing `(raw, li)`
  parameters, which is why no pixel statement moved.
- **The span guard** (`(sx0, sx1) = (0, 0)` unless `x0 < x1`) is the one
  place the canvas span meets the `u32` layer span; the test documents carry
  layers (and clip members under a styled and a knockout base) entirely
  right and left of the canvas.
- **Split 13B-2c** into 2c-1 (this run), 2c-2 (writers, job control on
  `to_contiguous`, tile-aware comparisons, asset-side tiles, the synthetic's
  seed) and 2c-3 (persistence / export / preservation / thumbnail reads).
- **Carried nits closed**: (1) `record_contiguous_views` split into
  `contiguous_views_of` + `record_contiguous_views`; (2) the report is a
  point sample (contract); (3) the eviction `scope` names the process-wide
  view drop and `evict_caches` walks once.
- **Knockout loop form** (implementation): the alpha read is `px[3]` from
  `for (i, px) in run.iter().enumerate()` rather than `run[i][3]` —
  `clippy::needless_range_loop` refuses the indexed form; it is the same byte.

### 13B-2c-2a decisions

- **Split 13B-2c-2** into 2c-2a (this run: the rect writers, `unseeded`, the
  carried nits), 2c-2b (job control on `to_contiguous`, tile-aware equality)
  and 2c-2c (asset-side tiles). `doc_geometry_set.rs:161` is a resample READ,
  not a comparison, so it moved to 13B-2c-3's residual list.
- **Inpaint narrowing (the one answer that moves).** A union window that
  misses the layer answers `Ok(None)` before any solve. HEAD still ran
  `hole_distance` / `inpaint_plane` per component and could `Err` from them
  (`inpaint_em.rs`'s no-eligible-source refusal, a cancel inside the solve,
  an allocation failure) before the write-back found nothing to write; those
  errors are no longer raised for a call that cannot move a byte. Measured:
  HEAD answers the test (g) fill with "there is nothing to sample from: every
  pixel around the selection is transparent, part of the selection, or
  outside the canvas."; this run answers NULL with no message. Every check
  ahead of the loop (`reach_x/y`, the hole cap, the source check, `reserve`)
  keeps its place and order.
- **Whole-image writers stay whole**: `with_layer_space_plane` (whole layer
  by contract), `gradient`, `apply_layer_pixels` / `apply_layer_plane`
  (full-layer results), `layer_via` (the copy IS the new layer), the mask
  apply, `doc_color_policy`, `ffi_doc::rz_doc_layer_image` keep
  `to_contiguous` + `with_layer_pixels` / `rebuilt`.
- **The deposit's target extent** (implementation): the retouch-output
  deposit writes a NEW canvas-sized layer from the caller's plate, so an
  off-canvas target still gets its layer (today's answer, kept); the suite
  records that per site rather than expecting NULL.
- **`LayerClip` loses its rect fields** (implementation): with the rect from
  `canvas_clip` nothing read `lx0 … ly1` any more (clippy's dead-field
  refusal), so `LayerClip` keeps its miss test and `canvas_index` only.
- **`layer_raster::ALLOCATION` is `pub(crate)`** (implementation): the one
  home of the allocation sentence, which the heal / inpaint rect reads raise.
- **No new checkpoint**, so no cancellation bound moves; the
  `content_aware_*` timing and `refinement_cutout` cancel suites are known
  load flakes and are reported, not chased.

### 13B-2c-2b decisions

- **`JobContext::cancelled`** tells a cancel from an allocation failure in
  `materialize_with_job`; a `job.check()?` after the failure was declined
  because it adds a checkpoint on the cancel path (the parked test's
  "exactly 194" would move).
- **Equality takes no job**: four of the five sites have no `JobContext`,
  and adding checkpoints to the artwork path would move its sequence for
  nothing. The only new checkpoints are the conversion's tile walk.
- **`attach_byte` compares before it builds**: an equal original is the
  layer's own raster (the `persisted_warp_native.rs:640/747` share), so no
  tiled copy is built only to be dropped.
- **`freezing_alpha` keeps `to_contiguous` + `rebuilt`** (a whole-image
  writer by 13B-2c-2a's decision; no job reaches `under_locks`); the only
  view it leaves is `rebuilt`'s seed of its answer.
- **The `set_layer_content_preserving_mask` view check** (implementation):
  a changed answer goes through `rebuilt`, whose seed is the answer's own
  view, so the suite asserts 0 `contiguous_views` for the SOURCE document
  (the comparison bound none) and exactly the one `w·h·4` seed process-wide.
- **The flip fixture's ruler origin** (implementation): a flip moves an
  off-centre ruler origin, so the mirror-symmetric document sets its origin
  at the canvas centre (`geometry_noops.rs`' `centered_byte` rule) to reach
  the NULL answer.

### 13B-2c-2c decisions

- **`Colored8`, not a tiled converted plane.** A converting `color8` answers
  an owned `RgbaImage`: tiling it with `LayerRaster::new` would seed a `w·h·4`
  view nothing reads whole (doubling a converting place's resident bytes) and
  leave dead seeds behind the preview, the Sample arm and the filtered read.
  It is tiled only where it is STORED, always `unseeded` (`into_raster`, the
  filtered cache entry). `LayerRaster::new` is called by no changed site.
- **A render always mints a fresh identity** (`reidentified` / `unseeded`),
  as `shared` did, so `Layer::effective_source`'s identity test and both
  identity caches see the same answers; the Place ADOPTS the input raster's
  identity (the asset IS the input layer's raster).
- **Two whole-plane binds** (the Sample-arm resample and the filtered read)
  are kept as `contiguous_image()` binds: the resampler and `filtered` read
  a contiguous plane, and rewriting either is out of scope. With the asset
  PNG writer's bind they are on 13B-2c-3's list; the residual is 25.
- **`color8_image`** keeps the contact sheet on an owned image (its
  flattened projection is not a raster), sharing `convert8_runs` so the
  per-pixel statement exists once.
- **The (d) oracle uses the public `Transform::new` + `apply`**
  (implementation): `Transform::between` / `Transform::color` are
  `pub(crate)`; `apply` runs the same `color` per pixel with alpha untouched,
  and Display P3 → sRGB is not `approx_eq`, so `between` answers `new`.
- **Reopen dedup is not this run's**: a reopened placed source decodes two
  equal planes (the layer PNG and the asset PNG, the format's own layout) —
  an open item for 13B-2c-3 (a `same_bytes` adopt at `materialize_byte`).

### 13B-2c-3a decisions

- **13B-2c-3 is split.** 3a (this run) takes the persistence / export /
  preservation / thumbnail / canvas-image / plate-copy / promotion reads and
  the reopen dedup; 3b takes the resample-input and temp-document reads and
  the seven unlisted residual sites (plan table).
- **The asset writer takes NO job-polled copy** (`to_contiguous`, not
  `_with_job`): the PNG encode that follows is itself uncancellable and
  dominates, so per-tile polling in the copy would change no measurable
  bound. No new checkpoint → no new bench probe.
- **Both readers build `unseeded`**: after this run nothing reads a freshly
  opened layer or WARP original whole (compositor: tiles; thumbnail: a
  transient copy; writer: `runs`; promotion: `promote_raster`; the WARP
  original is otherwise read only by `effective_warp`'s `ptr_eq`).
- **A dabbed placed source reopens with 0 shared tiles.** Measured
  (suite (i)): the dab breaks `effective_source`'s `ptr_eq`, so the writer
  drops that layer's binding and its asset — the reopened layer carries no
  retained source at all, and there is nothing to share with. Partial
  re-sharing through `rebuilt_from` over an asset that IS persisted (a
  binding that survives an edit) is an open item, owner 13C.
- **The test oracle is a binding-aware contiguous twin** (every raster
  `shared`, `retained_source.rendered` / `warp.rendered_pixels` re-pointed,
  each asset Arc re-adopted once through a memo keyed by `Arc::as_ptr`, the
  WARP original re-adopted contiguously); each twin save asserts its `RSRC`
  / `WARP` tag so the stream equality is not vacuous. Post-call view and
  sharing checks are collected and reported together, so a run against the
  HEAD library names every failing check.
- **`layer_raster_writers.rs`'s pinned fill view** (`Site::views`, the one
  sampling-plate view 13B-2c-2a recorded) is now 0 for every site: the plate
  copy reads rows.

### 13B-2c-3b decisions

- **A tap-walk input is a transient copy, not per-tap tile reads.** The
  resample walks read the source at random 2-D offsets; a per-tap
  `sample_at` would put a division and two indirections into the interactive
  transform inner loop, which 13A-latency owns and this run must not worsen.
  One owned copy per operation keeps the loops unchanged and leaves no view.
  Per-tap tile reads (a scratch reduction) are 13C / 13A-latency territory.
- **The accepted cost is the warm path.** At HEAD a repeated edit on the
  same input plane (a second `warp_edit` re-reading `binding.source_pixels`;
  a second live-filter edit re-reading the asset raster in `source8` on the
  masked-cache miss) hit the view cache — an `Arc` clone. After this run it
  pays a full transient copy per edit. Measured, not argued
  (`phase13b_tile_sharing` `transforms/warm`, 3000×2000): `warp_edit#2`
  699.6 ms BEFORE → 686.2 ms AFTER, `live_filter_edit#2` 272.3 → 277.1 ms —
  one sample each; the live-filter delta is of the order of one 24 MB copy
  (against a 0.3–0.7 s edit); the
  views those pairs held, 72064016 and 24000000 bytes, are now 0. Any
  mitigation (a transient-copy memo, a per-binding held copy) goes to
  13A-latency with this figure.
- **The warm pairs run at 6 MP, not 24 MP.** A bilinear warp charges
  `w·h·taps²·64` = `w·h·256` work units, so 24 MP exceeds `MAX_WORK_UNITS`
  (2·10⁹, "warp exceeds the work-unit limit"), and a σ 2 / σ 3 Gaussian
  source filter exceeds its pixel-work cap at 24 MP. Both pairs use a
  half-size 3000×2000 corpus plane; the cold rows stay at 24 MP.
- **Both temp documents are `unseeded`.** Nothing reads a temp layer whole
  (heal / spot / inpaint are rect readers, the lift reads runs, the deposit
  reads its domain rect), so a seed would only evict a live view.
- **(h) and (k) are checked by `Checks::seeded`, not `no_view`.** Their
  outputs seed by design (`rebuilt` — the twin's `rebuilt` on a contiguous
  raster is `new` — and `with_layer_pixels`), so the check is "no view of an
  input plane, and exactly the outputs' seeds process-wide": `2 · 640·480·4`
  each, exact because `reserved_vec` is `try_reserve_exact` and
  `RgbaImage::new` is exact-sized.

### 13B-3a decisions

- **Rename, not overload.** A same-named `as_rgba16` returning an `Arc`
  would still compile inside loops and closures and hide every per-sample
  call; the new name forces every site to be read (and the bind-once review
  to see it).
- **`ptr_eq` keeps storage in the answer**, so a u16 and an f32 image can
  never be "the same" even through a future shared representation.
- **`same_samples` is bitwise on f32** (`Sample::same_bits`), the latch the
  tiles use in 3b, so the two equality sites will not change meaning at the
  flip; u16 is plain `==`.
- **`to_rgba16/32` were added** for 3b's handle-owned view and for the test
  copies (`.to_vec()` sites), so neither has to bind a view to copy.
- **No `contiguous_*` cache yet**: in 3a a view is an `Arc` clone; the typed
  view cache arrives with the tiles in 3b.
- **Constructor validation stays after the reservation** (`from_rgba32`,
  `from_rgba32_extended`): moving it before `out` would change the checkpoint
  order the cancellation tests pin; the new suite states the order with a
  NaN buffer under a sub-bookkeeping budget.
- **`alpha16` reads `pixel(index)`**: it is the one per-sample reader, and a
  bound view there would be a per-sample `Arc` clone; the round trip is exact
  for every u16 code.
- **`precision_color.rs`'s shared-allocation dedup** (`seen` / `outputs`
  keyed on `view.as_ptr() as usize`, lines 112 and 340) is a raw-address
  key inside `core/src`; in 3a it is the image's own allocation, so it is
  unchanged — a 3b obligation beside the six test oracles.

### 13B-3b-1 decisions

- **The split.** 13B-3b's spec split the run: the flip, the shared cache,
  the handle-owned lend, the transient term and the oracle migration are
  13B-3b-1; `rebuilt` / `with_rect` / `unseeded` / `reidentified` with
  their first callers (`render_high_depth`'s Shared arm, the whole-image
  typed writers) are 13B-3b-2, and the "typed dab ≤ 4 tiles" figure is
  13B-3d's.
- **(a) The constructors' `admit` figure is the CONTIGUOUS one**
  (`capacity·bpp` + profile, taken before `tile()`); the tile index (8 B per
  tile) is not admitted, and neither is the tiling copy itself (the
  transient is planned only where `work_plan` sizes a render).
- **(b) A tiled plane's `storage_bytes` / `precision_pixels` is `w·h·bpp`.**
  An adopted Vec's slack capacity now shows up only as the seeded view's
  `contiguous_views` bytes (`View::bytes()` is capacity-based): a 1×1 image
  from a 1024-capacity Vec is `storage_bytes() == 8` with an 8192-byte view.
- **(c) Planes above `CONTIGUOUS_CACHE_BYTES` stay Contiguous** (an f32
  plane above 67,108,864 px): the cache would refuse their view, so every
  residual bind would be an un-admitted whole copy and
  `precision_geometry.rs:203`'s short-circuit could never fire. They tile
  once 13B-3c/3d/3e move the reads off whole views; the view bound itself is
  13B-6's.
- **Every constructor seeds** (2b's `new` policy). A producer whose plane
  nobody reads whole (the 16-bit structure promotion, a render nobody binds)
  therefore leaves a view until 13B-3b-2's `unseeded`;
  `layer_raster_persistence`'s promotion check now asserts every view left
  is the promoted document's own (no BYTE view).
- **One cache for both depths**, keyed by variant + index: a byte and a typed
  plane can never alias, the bounds are shared, and the report / eviction
  / host trim code needed no second path.
- **The pin is a `OnceLock` in the image**, so the lend is valid for the
  handle's life with no FFI signature change and a clone does not inherit
  (and so cannot extend) another handle's pin.

### 13B-3b-2 decisions

- **`rebuilt` seeds.** The typed compositor still binds whole views until
  13B-3c, so the next projection reads the edited plane whole; the seed is
  the buffer the writer already built (no copy) and saves that bind a
  materialization. A no-change rebuild seeds under the SAME plane key, so
  the insert replaces the live entry — never two views of one plane.
- **`with_rect` never seeds** (the byte rule): it holds only the rect, and a
  whole-plane view would cost the copy the rect write exists to avoid.
- **`rebuilt_*` take no `control`.** All eleven sites passed `None`, and the
  diff walk (`TilePlane::rebuilt_from(.., None)`) polls nothing, so no
  checkpoint moves and no cancellation sequence changes.
- **`precision_retouch`'s in-place arm rebuilds from `original`** and so
  shares every unchanged tile when the extent did not grow (extent
  dimensions == `original.dimensions()`); a grown extent
  (`expand_destination`) re-tiles fully through the fallback. The
  `NewLayer` arm keeps `from_rgba16` (a new layer has no prior plane), so
  the single construction moved into the two arms.
- **The lock restore rebuilds from `src`** (the retained state the restore
  copies from), never `dst`: the restored plane is `src` everywhere the
  lock froze it, so that is where the tiles are shared.
- **`reidentified` changes no `core/src` answer.** `retained_source.rs:341`
  and `precision_warp.rs:33` compare the layer against `rendered`, and both
  are the render's clone (the same identity); `develop_inspect`'s master is
  the asset, which is untouched. The `ptr_eq` oracles that compared a placed
  LAYER with its master are none (measured: `retained_sources`,
  `placement_insert`, `develop_inspection`, `phase8e_native`,
  `selected_adjustments` all green unchanged).
- **The split uses `#[path]` child modules**, so the representation stays
  private to `precision_image` and its children (fields private, plane types
  `pub(super)`), rather than `pub(crate)` fields in sibling crate modules.
- **`from_rgba16_unseeded` for the promotion and the placeholders only.**
  Reader-side `unseeded` (`precision_png`, `precision_codec`,
  `psd_import16`, `float_io`, `float_native`, `warp_native`, the clipboard
  codec) is 13B-3e's, with the typed export reads it would otherwise
  immediately re-materialize for.
- **Constructor site classification** (the 62 HEAD `PrecisionImage::from_*`
  / `from_float_samples` / `from_rendered_float` call lines, `convert`'s
  included): `rebuilt` 11 and `unseeded` 8 (above, this run); READERS →
  13B-3e (12: `precision_png.rs:153`, `precision_codec.rs:495`,
  `psd_import16.rs:281, 427`, `float_io.rs:158`, `float_native.rs:128`,
  `warp_native.rs:530`, `precision_clipboard_codec.rs:313, 545, 568`, and
  the host intakes `ffi_precision_input.rs:76`, `host_pixels.rs:236`);
  renders, exports, resamples, new layers and transient scalar images →
  13B-3c (29: `precision_render.rs:766, 775`, `precision_regional.rs:230,
  239`, `precision_style_render.rs:868`, `artwork_render.rs:142`,
  `live_filter_evaluate.rs:396, 423`, `raw_local_develop.rs:264`,
  `retained_source_render.rs:271`, `precision_retouch_source.rs:64`,
  `doc_export.rs:1116, 1328`, `doc_export_layers.rs:134`,
  `precision_sampling.rs:87, 107, 200`, `precision_warp.rs:117`,
  `raw_source_geometry.rs:466`, `ffi_retouch.rs:46`,
  `precision_paint.rs:300`, `precision_insert.rs:132, 141`,
  `precision_clipboard.rs:193, 213`, `precision_clipboard_convert.rs:117`,
  `precision_scalar_filter.rs:53, 156`, and `convert`'s cross-storage
  `from_float_samples`); the `from_float_samples` WRITERS
  `precision_adjustment.rs:296` and `float_document.rs:620` (the rebuild
  twin they need) → 13B-3d.

### 13B-3c-1 decisions

- **13B-3c is SPLIT.** 13B-3c-1 (this run) moves the typed compositor, the
  regional typed tree and the geometry comparison onto the tiles. 13B-3c-2
  owns the rest: the projection OUTPUT's own seed (the render / regional /
  style-bake constructors unseeded), which first needs every consumer of a
  projection handle on `runs` / `sample_at` (`quantize` /
  `quantize_scratch_bytes`, `tone_mapped_u16`, `precision_stats`,
  `float_stats`, `soft_proof`, `ffi_evaluation`'s compare,
  `precision_scoped_source`), `precision_sampling`'s resample-input binds,
  then 13B-3b-2's 29-site constructor seed classification.
- **The 3c row's "persistence reads" are 13B-3e's** (that row owns `.rz` /
  PSD / export reads); nothing persistent is read here.
- **No typed content-box scan exists.** `scan_content_box` is byte-only
  (`doc_group_query.rs`, moved in 13B-2c-1); the 3c row's "content-box" has
  no typed site.
- **Carried nit 1 stays 13B-3d's**: the contiguous branches of
  `Plane::with_rect` / `read_rect` get their coverage with their production
  callers; this run adds no rect read.
- **The runs keep the 4096 chunk.** The raster arm still advances progress
  per 4096 canvas columns and reads each chunk's layer-space span, so a
  chunk boundary may fall mid-run (the 4400×40 parity fixture pins it); the
  progress / cancellation sequence is HEAD's.
- **`image_equal`'s identity short-circuit is `plane_key()`.** HEAD compared
  the two bound views' `as_ptr`, which is equal exactly when both images
  share one plane (one cached view per plane, or one contiguous `Arc`);
  `plane_key()` is that plane's key with no view bound.
- **Blend If and styles have no float fixture.** Both are refused at float
  (`float_document`'s style sentence), so `precision_compositor_parity`'s
  Blend If, styled, knockout and styled-group fixtures are u16 only.
- **The float parity fixtures take their rasters by Copy + Paste in place.**
  A float document is admitted no per-raster constructor
  (`PrecisionDocumentLayer::raster` refuses f32, `raster_float` is
  `pub(crate)`), so each raster is copied out of a padded 1800×1600 float
  TIFF source and pasted in place, and the admitted float `crop` sets the
  offsets (the source itself ends up off every edge); the wide fixture's
  offset is a float `canvas_size`. `precision_image_reads` (e)'s float
  document uses the admitted integer `affine` translation (`move_layers`'
  form) for its third raster.

### 13B-3c-2a decisions

- **13B-3c-2 is SPLIT** (the run's own instruction): 13B-3c-2a (this run)
  moves every consumer of a projection handle onto the tile reads;
  13B-3c-2b unseeds the projection OUTPUT after classifying the other
  projection readers the survey found; 13B-3c-2c owns `precision_sampling`'s
  resample-input binds and the rest of 13B-3b-2's 29-site seed
  classification (plan table).
- **`rgba16_at` is new rather than `sample_at` + a re-quantization.** The
  stats sample and the scoped analysis report exact u16 CODES; recovering a
  code from `normalized16`'s f32 would be a second rule. `rgba16_at` is the
  plane's stored sample, `Plane::sample_at` verbatim.
- **Every storage gate is an explicit `storage()` test with HEAD's
  sentence, in HEAD's position** (before the reach check, after the
  Proofer, after the dimension check), because the walk no longer produces
  the `Option` the old `ok_or` read; where a `runs_*` `Option` is in hand
  (tone map, histograms, proof, compare) it is the gate itself.
- **The report index is reconstructed, not re-counted.** The quantizer's
  flat `i = y·w + x` and the histograms' `i = visited·step` are exactly the
  values HEAD's index loops reported, so the `i & 4095` / `visited & 4095`
  cadence and each report's fraction are unchanged (the `consumer` lines'
  `checkpoints=` and `reports=` hashes pin it at HEAD and on the tree).
- **The quantizer's `PrecisionRun::Rgba32` arm is a `continue`, not a
  panic.** The storage gate above it makes it unreachable (a u16 plane
  answers only u16 runs); a `continue` keeps an FFI path panic-free.
- **The float display's intermediate is left seeded.** `tone_mapped_u16`'s
  output is `from_float_samples`'s (seeded) plane; it is dropped inside
  `display_rgba8` and its seed pruned with it, so `display_rgba8` leaves 0
  views (measured). Unseeding it is 13B-3c-2c's re-classification.
- **Measured cost of the stats walks, recorded, not bounded.** The spec
  fixed a bound only on the interactive display paths (warm
  `preview_rgba8` and `display_rgba8` ≤ 1.10 × HEAD, both met). The first
  pass's `flatten()` walks were slower warm than HEAD's indexed slice
  (compare ~3.7 ×, u16 histogram ~1.45 ×, float histogram ~1.14 ×); the
  review fix pass rewrote compare (cursor-paired run zips) and both
  histograms (tile-band walk, hoisted reports) with identical answers,
  checkpoints and reports, re-measured at compare 0.54 ×, u16 histogram
  1.07 ×, float histogram 1.00 × warm. The one consumer left slower is the
  scoped Layer histogram (~1.15 ×): its cost is a per-index `rgba16_at`
  inside `scoped_statistics::evaluate`'s `|i|` callback, a shape the spec
  fixes; an open item for 13A-latency.

### 13B-3c-2b decisions

- **The classification** (the parameter is required, so the compiler
  forced a choice at every call site):

  | Site | View | Reader / reason | Owner of the flip |
  | --- | --- | --- | --- |
  | `precision_document.rs` `render` / `precision_render.rs` `render` | Unseeded | the host handle; its whole-plane readers are the exports (`precision_codec.rs:742`, `doc_export.rs:1096/1317`, `precision_sampling.rs:90`, `float_io.rs:181`, `psd_export16.rs:499`), each paying ONE materialization per handle, then the cache (measured: `typed/unseeded` `export_*` / `save_psd`) | — |
  | `precision_regional.rs:230/239` | Unseeded | display | — |
  | `precision_conversion.rs:258` | Unseeded | quantize / tone map (tile reads since 13B-3c-2a) | — |
  | `float_document.rs:675` | Unseeded | tone map | — |
  | `precision_clipboard.rs` Copy Merged, `precision_conversion_narrow.rs` ×2, `precision_conversion_structure.rs` | Unseeded | moved onto runs / rows in this run | — |
  | `precision_scoped_source.rs` `scoped_analysis` | Unseeded | the `rgba16_at` closure | — |
  | `precision_color_policy.rs` preflight | Unseeded | the opacity scan; the output is dropped | — |
  | `precision_merge.rs:300/360/389/495` | Seeded | the layer it installs; the next whole-layer writer (e.g. `precision_paint.rs:330`) | 13B-3d |
  | `precision_color_policy.rs` `bake_adjustment` | Seeded | the layer; `convert_pixels` at :259 | 13B-3d |
  | `precision_retouch_source.rs:101` | Seeded | the plates: `precision_retouch.rs:388/427`, `precision_inpaint.rs:189`, `ffi_retouch.rs:43` (random-access bilinear taps) | 13B-3c-2c (with the Current arm at :64) |
  | `precision_selection.rs:125` | Seeded | the `\|i\|` closure at :132 | 13B-3c-2c |
  | `doc_export.rs:1270`, `doc_export_layers.rs:311`, `precision_io.rs:122` | Seeded | export chains | 13B-3e |
- **D1 — the style bake stays seeded.** `precision_style_render.rs:919`'s
  only reader, `psd_export16.rs:363`, binds it whole, so it moves with that
  reader to 13B-3e. This deviates from the plan row, which listed it among
  the constructors to unseed.
- **D2 — layer-producing renders stay seeded.** HEAD's first whole-layer
  write after a merge or flatten hits the seed; unseeding would add one
  plane copy to an interactive dab (13B-3b-2's `rebuilt` rationale).
  `seeded_outputs_keep_their_view` pins it (`flatten`, `merge_visible`,
  `stamp_visible` each leave exactly `W·H·8` over the result).
- **Only the export readers pay.** The one new cost is one materialization
  per host-projection handle, paid by the first export-side whole-plane
  read of it: `typed/unseeded` `export_png16` `ms_first` 1.31–1.33 × HEAD
  (a 24 MP u16 view built from the tiles, ~30 ms), `ms_warm` at parity;
  the other export / save lines are within 1.05 × (recorded, no bound).
- **The preview fixture's `Rgba16` arm uses alpha 0 or 1.** At a
  fractional alpha the u16 twin's own composite re-derives straight RGB and
  differs from the tone-mapped reference by one code (max_delta 1 at
  600×520), so the spec's "delta 0" holds only for alpha in {0, 1}; the
  suite states it and keeps the assertion.

### 13B-3c-2c-1 decisions

- **The classification** (View / reader / owner of any later flip; HEAD
  line numbers):

  | Site | View | Reader | Owner |
  | --- | --- | --- | --- |
  | `precision_retouch_source.rs:65` Current arm, `:102` cut render | Unseeded | the host's `previewRGBA8` (quantize rows), `ffi_retouch.rs:43` (transient), the backdrop (rows), `precision_retouch.rs:427` (transient), `precision_inpaint.rs:189` (rows) | — |
  | `ffi_retouch.rs:46` | Unseeded | the host's `previewRGBA8` (`EditorViewController+Heal.swift:94`) | — |
  | `precision_selection.rs:126` `analysis_source` | Unseeded | `canvas_samples` rows | — |
  | `precision_sampling.rs:87/107` | Seeded | the layer `precision_geometry` / `render_high_depth`'s Sample arm installs; the ≤ 1 MP `develop_detail` host handle | 13B-3d |
  | `precision_sampling.rs:200` | Seeded | the layer; `doc_export.rs:1318`'s sharpen / stage 3 | 13B-3d / 13B-3e |
  | `tone_mapped_u16` (`float_document.rs:621`) | the caller's | Unseeded: `display_rgba8` :653, `downconvert` :266, the narrow preview :758, `narrow_to_byte` :229 (quantize / runs), and `export_stages` :1300 when `(w, h) != source.dimensions()` (its only reader is `resize_image`'s transient, `precision_sampling.rs:182`). Seeded: `float_to_u16` :678 and narrow-to-u16 :449 (the layer), and `export_stages` :1300 when `(w, h) == source.dimensions()` (`resize_image`'s clone at :178 → `:1318`'s sharpen / stage 3) | 13B-3d (the layers) / 13B-3e (the export) |
  | `precision_retouch.rs:549` NewLayer | Seeded | the new layer's next whole-layer writer | 13B-3d |
- **The `export_stages` size split.** A float export's tone map seeded a
  view that a RESIZING export never reads once `resize_image`'s bind is a
  transient copy, so a resizing export would have held tiles + seed +
  transient. `:1300` therefore passes `Seeded` only when the export keeps
  the source's dimensions (the sharpen and stage 3 then bind the clone
  whole, as at HEAD) and `Unseeded` otherwise, so a resizing export holds
  tiles + one transient, as HEAD held tiles + one view. Admission and
  plans are unchanged: in peak terms the transient replaces the seed.
- **One transient per operation, never per-tap tile reads** (13B-2c-3b's
  rule): the bilinear / bicubic tap walks read an indexed slice; a
  contiguous plane is borrowed, not copied. The accepted cost is the WARM
  path: a repeated resample of one input copies the plane per call where
  HEAD hit the view cache — `typed/plates` `geometry_rotate90_float`
  (3000×2000 f32, RECORDED) warm 52.4 → 63.4 ms (1.21 ×), the other
  resample lines within 1.03 × (below). No transient memo (out of scope).
- **The retouch plates never needed a view.** Their readers are row copies
  (the host's quantize, the backdrop), one window walk (the inpainter) and
  two bilinear tap walks (the source transform, the clone/heal source) —
  each now reading rows or one transient. The Current arm kept its
  checkpoint sequence by splitting HEAD's interleaved loop into the
  per-row `job.every` pass and the row copy (the copy polls nothing).
- **`retouch_clone_new_layer` runs at 3000×2000 in the example** at HEAD
  and on the tree alike: at 24 MP its admission needs 3138888832 bytes
  (limit 2147483648), a refusal this run does not move.

### 13B-3c-2c-2 decisions

- **The rule.** Unseeded only where every reader is view-free after at most
  a sequential-walk move. Layer producers stay Seeded (13B-3c-2b D2). Any
  other remaining whole reader keeps a site Seeded, with that reader named.
  The rule is why the export sites stay Seeded, with no per-request split.
- **The classification** (HEAD `71e4873` line numbers; flip owner 13B-3d
  when every remaining whole reader is a whole-layer writer, otherwise
  13B-3e, whose exit condition is residual 0):

  | Site | View | Readers | Flip owner |
  | --- | --- | --- | --- |
  | `artwork_render.rs:142` | Seeded | The typed publish installs it as the layer (`artwork_document.rs` `artwork_unlocked`, `with_profile`). Its next whole-layer writer is `precision_paint.rs:330`. The byte publish reads quantize rows. Replace reads `same_samples`. | 13B-3d |
  | `live_filter_evaluate.rs:396` | Seeded | `retained_source_render.rs:251` (`color16` input), `live_filter_evaluate.rs:409` (`filtered`), `live_filter_preview.rs:160`, and the placed layer (`render_high_depth` `reidentified`) | 13B-3e |
  | `live_filter_evaluate.rs:423` | Seeded | The live-filter cache entry, `live_filter_preview.rs:160` and the placed layer. `transform_image` reads a transient. `render8` reads quantize rows. | 13B-3e |
  | `retained_source_render.rs:271` | Seeded | `live_filter_evaluate.rs:409`, `live_filter_preview.rs:160` and the placed layer | 13B-3e |
  | `raw_local_develop.rs:264`, `raw_source_geometry.rs:466` | Seeded | The layer of the one-layer develop document. The next develop stage reads `raw_source_geometry.rs:392` (`RawDevelopmentRecipe.swift:433-437`), and `raw_local_develop.rs:163/183` read it. As a RAW master it is read by `develop_inspect.rs:134` and `retained_source_render.rs:251`. | 13B-3e |
  | `doc_export.rs:1117` | Seeded | 16-bit: `precision_codec.rs:742`. 8-bit: quantize rows. | 13B-3e |
  | `doc_export.rs:1334` | Seeded | `doc_export.rs:1097` (`convert16`) and `precision_codec.rs:742`. 8-bit without conversion: quantize rows. | 13B-3e |
  | `doc_export_layers.rs:135` | Seeded | `export_stages` → the clone in `resize_image` → `doc_export.rs:1323/1097`, `precision_codec.rs:742` | 13B-3e |
  | `precision_warp.rs:117` | Seeded | The layer and its `rendered_pixels` (same identity) → `precision_paint.rs:330` | 13B-3d |
  | `precision_paint.rs:300` (Rasterize) | Seeded | The new layer → its next whole-layer writer | 13B-3d |
  | `precision_insert.rs:132/141` | Seeded | The new empty layer → its next whole-layer writer (with the `TilePlane::filled` item) | 13B-3d |
  | `precision_clipboard.rs:226/262` | **Unseeded** | RZCB `encode` and `converted` (both on runs since this run). The host's `publicRendering` → `display(of:)` (quantize rows / `display_rgba8` runs). Only when an in-memory FFI handle is passed directly: the no-convert arm of `clipboard_paste` (`precision_clipboard.rs:622`, `with_profile` installs it as the layer; reached through `ffi_clipboard.rs:185`) and `from_clipboard` (`precision_clipboard.rs:640`, `clone` into the Background record; reached through `ffi_clipboard.rs:401`). | — |
  | `precision_clipboard_convert.rs:117` | Seeded | `clipboard_paste` installs it as the layer → its next whole-layer writer | 13B-3d |
  | `precision_scalar_filter.rs:53` | Seeded | `selected_filter.rs:263` (Pixelate) and `selected_auto_levels.rs:52` (auto arm), otherwise `pixel(i)` | 13B-3e |
  | `precision_scalar_filter.rs:156` | Seeded | `selected_auto_levels.rs:52` | 13B-3e |
  | `precision_image.rs:538` (`convert`) | **Unseeded** | No `core/src` caller. The tests read owned copies. | — |
- **The Copy / Cut payload is not a layer producer under D2.** The host
  pastes and makes documents only from DECODED payloads
  (`ClipboardPayload.swift:74/94` → `decode`, whose constructors at
  `precision_clipboard_codec.rs:545/568` stay seeded) or from `bytePayload`
  (`ClipboardBoundaries.swift:100/332/422` and `AgentServer+Clipboard.swift`
  paste only `TypedClipboardPayload.handle` or `bytePayload`). An unseeded
  layer therefore arises only when an in-memory FFI handle is pasted
  directly (tests: `precision_projection_unseeded::float_multi_document`
  and the float fixtures of `precision_compositor_parity` now hold unseeded
  layers, their answers unchanged). That layer's next whole reader (for
  example `precision_clipboard.rs:492`) materializes then; 13B-3e owns
  that.
- **The `CONTIGUOUS_CACHE_BYTES` share needs no tiling change**, because
  `tiles_of` refuses above the bound for both tilings.
  `precision_clipboard.rs:262` (now unseeded) keeps the contiguous
  fallback; its readers' runs serve it in place. `precision_insert.rs:132`
  and the f32 target of `convert` keep the fallback too. f32 whole readers
  remain (`precision_clipboard.rs:492`, `float_native.rs:73`,
  `float_io.rs:181`, `precision_lock.rs:289/291`), so the item's owners
  become 13B-3d / 13B-3e.
- **`encode` walks `run.as_flattened()`** inside the per-run loop rather
  than a third per-pixel loop: the same component order and bytes, and the
  codec file keeps no net growth (651 lines).

### 13B-3d-1 decisions

- **The split.** 13B-3d as planned (every typed rect writer plus every
  Seeded layer producer) is too large for one run: 13B-3d-1 takes the
  PAINT writers (the interactive path the criterion measures), 13B-3d-2
  the retouch / scalar / lock-restore / whole-image writers' binds and the
  `from_float_samples` twin, 13B-3d-3 the Seeded producer flips (plan
  table).
- **Mirror 13B-2c-2a, do not re-derive.** The rect bounds the READ; the
  bitwise diff in `with_rect` is the changed latch and bounds the copies,
  so HEAD's separate changed scan is dropped (identical answers: every
  sample a walk can change lies inside its rect).
- **One implementation of the clip.** The typed writers use plane forms of
  `layer_rect`'s helpers rather than a typed copy; the byte forms keep
  their signatures and delegate, so the `layer_raster_writers` suite
  guards both.
- **Heal over an empty plane** when the window misses the layer, instead
  of an early `Ok(None)`: HEAD ran the whole solve (and could refuse from
  it) for such a window, so the answer and every refusal stay HEAD's.
- **The native tools' GROWN extent keeps the whole rebuild** (and its
  `nw·nh·8` seed): a grown layer has new dimensions, so there is no plane
  to `with_rect` into; it is filled from rows (no view bind) and owned by
  13B-3d-3.
- **Checkpoints shrink with the rect** (the only intended difference): a
  dab costs the same checkpoints on a 4× plane for every site but bucket
  fill (its grow is canvas-wide) and the Cut (its Copy is window-wide).
  No checkpoint site is added, so no cancel-bench probe is added.

### 13B-3d-2 decisions

- **Not split.** The row's writers fit one run: the in-place retouch arm and
  the lock restore become rect writers, the rest keep their whole-image
  shape and only change how they READ.
- **Split the retouch dispatch at the destination write** rather than
  re-derive each op's rect: `prepare` / `apply` are HEAD's statements moved,
  and `bound` states per op what the write can touch (Heal's `+1` is the
  component windows' padding; synthesis uses the plan's own windows). The
  in-place arm mirrors 13B-3d-1's heal: a bound missing the layer runs the
  write over an EMPTY plane, so every solve and refusal stays HEAD's.
- **The not-in-place arm keeps the whole extent buffer**: a new layer and a
  grown extent are new planes with nothing to `with_rect` into; only its
  Layer fill moves from the view to rows. Their outputs keep seeding
  (`from_rgba16` / `rebuilt_rgba16`; owner 13B-3d-3).
- **The lock restore bounds itself by tile SHARING**, not by the edit:
  `freezing_alpha` sees only `src` and `dst`, and every rect writer's `dst`
  shares all but its rect's tiles with `src`, so the unshared bbox is the
  edit's tile-aligned rect — and a whole-image writer's `dst` (which shares
  nothing it changed) restores over the whole plane, exactly HEAD's work.
  No caller changes.
- **Rows / runs + `rebuilt_*` for the whole-image writers**, in PLAIN nested
  loops: 13B-3c-2a measured iterator chains over the run stream at up to
  3.7 × HEAD, so no `.flatten()` / `.enumerate()` / `.zip()` over runs; the
  index is taken at the top of the body so a `continue` / `?` keeps HEAD's.
  The adjustment's changed scan and `precision_scalar_edit::read()` walk in
  TILE-BAND order (a row across every tile column cost 1.04–1.06 × HEAD in
  the first cut, the shape 13B-3c-2a measured at ~20 %).
- **`rebuilt_float_samples` for the destructive adjustment only**; the
  f32 → u16 tone map shares no tile with its output (another storage), so
  it needs no twin. A NO-CHANGE destructive adjustment leaves the target's
  `w·h·8` view cached, as HEAD `a33e203` did — there its changed scan bound
  that same view; here `rebuilt_*` seeds its buffer under the TARGET's own
  plane key (`rebuilt_from` answers the source plane) — so the footprint is
  the same and only the holder moved (bounded, evictable;
  `no_view_retouch_writers` allowed it). Since 13B-3d-3a no view at all
  (`rebuilt_*` seeds nothing).
- **The f32 destructive adjustment refuses by name** where HEAD's view
  `unwrap` panicked: the storage check refuses f32, and the refusal
  sits where the panic was (after the output is built).

### 13B-3d-3a decisions

- **The split.** 13B-3d-3 as planned also needs NEW constructors
  (`PrecisionImage` / `LayerRaster` filled planes over `TilePlane::filled`)
  and a per-caller `OutputView` on `precision_sampling`'s resample outputs;
  13B-3d-3a takes only the flips an existing constructor or tiling policy
  already expresses, 13B-3d-3b the rest (plan table).
- **Change the `Rebuilt` arm, add no variant** (the `Tiling` one-body
  pattern): deleting the two seeds flips all eight `rebuilt_*` callers at
  once, and `Tiling` polls nothing, so no checkpoint, report or refusal
  moves.
- **Classification** (site → change → readers):

  | Site | Change | Readers of the installed layer |
  | --- | --- | --- |
  | `rebuilt_*` ×8 (scalar edit, mask apply, selected adjustment, `convert_pixels`, `precision_color`, the destructive adjustment incl. its no-change answer, retouch grown extent, native-tool GROWN extent) | `rebuild_over` seeds nothing | rect / runs writers, compositor, regional window; whole: the 13B-3e residual |
  | merge / stamp / flatten (`precision_merge.rs:301/361/390`), members render (`precision_document.rs:911`) | `OutputView::Unseeded` | as above |
  | the bake (`baked_projection`) | always `Unseeded` | `convert_pixels`' runs, then as above |
  | artwork (`artwork_render.rs:142`) | `from_rgba16_unseeded` | `artwork_unlocked`'s `same_samples`, the byte publish's quantize rows, then as above |
  | warp output (`precision_warp.rs:117`) | `from_rgba16_unseeded` | the layer and `rendered_pixels` (`effective_warp`'s `ptr_eq`); a later warp re-reads `source_pixels` (`precision_warp.rs:85`, 13B-3e) |
  | Rasterize (`precision_paint.rs:305`), paste convert (`precision_clipboard_convert.rs:123`), retouch NewLayer (`precision_retouch.rs:446`) | `from_rgba16_unseeded` | as above (paste: the RZCB `encode` walks runs) |
  | `float_to_u16`, the u16 narrowing (`leaf_u16`) | `tone_mapped_u16(.., Unseeded, ..)` | as above |

- **Carried nit 2 DECLINED** (`precision_scalar_edit.rs:217-218`'s
  `out.resize(w·h, 0)` zero fill): the tile-band walk writes out of row
  order, so a safe `extend` needs the row-order walk 13B-3d-2 measured
  slower (1.04 × vs 0.85 × HEAD on `typed/scalar_dab`), and
  `spare_capacity_mut` + `set_len` would be the crate's first non-FFI
  `unsafe`. The code is unchanged.
- **The warp arm keeps its SOURCE's view**: a non-translation warp binds
  its source whole (`precision_warp.rs:84-86`, `contiguous_rgba16()` caches
  under the source plane's key) and that plane stays alive in the input
  and the answer's `source_pixels`; only the OUTPUT seed is this run's, the
  source read is 13B-3e's.

### 13B-3d-3b decisions

- **D1 — not split.** The filled constructors, the per-caller resample
  view and the `encode` run walk are one slice: the walk is the only read
  change and it has no new constructor.
- **D2 — the filled constructors live in a new child `fill`**, because
  `precision_image_build.rs` was at 595 lines (the ~600 soft cap); `build`
  exports `assembled` and `EXTENDED` to its sibling as `pub(super)` (597
  lines after).
- **D3 — no admission in the filled constructors.** At ≤ `MAX_PIXELS` the
  adopted path's `w·h·bpp + 24 + BOOKKEEPING_BYTES` ≤ 1601048600 <
  `MAX_WORKING_BYTES`, so it never refused; the refusals are
  `pixel_count`'s sentence, `EXTENDED` and `ALLOCATION`.
- **D4 — retained accounting drops, on purpose.** A filled plane retains ≤
  4 unique tiles + the index, so `retained_bytes`, the report's
  `precision_pixels` / `layer_pixels` / `unique_retained_bytes` /
  `shared_bytes_saved`, and a later operation's admission `retained` input
  are smaller for a document holding an untouched filled layer — the
  deliberate, measured change of this slice. The plan charges
  (`StructureEdit::scratch_bytes`' AddLayer `w·h·bpp`, `walk.shape`,
  `walk.output`) stay HEAD's (13B-8). No existing test pinned an affected
  figure (the conditional row of the spec was empty).
- **D5 — each filled construction is its own plane**: structural sharing
  within the plane only, no interning across calls.
- **D6 — `doc_export.rs:1316` stays Seeded**, its readers named: the
  sharpen (`doc_export.rs:1329`) and `convert16` (`:1097`) bind it whole
  (13B-3e).
- **Classification** (site → view → readers):

  | Site | View | Readers |
  | --- | --- | --- |
  | `add_layer_at` (`precision_insert.rs`) | filled (none) | rect / runs writers, compositor, regional window, `encode`; whole: 13B-3e residual |
  | sourced shape (`precision_conversion_structure.rs`) | filled (none) | dimensions only (`structure_document`, `build`, `native_fits_geometry`) |
  | narrowed masked-adjustment plane (`precision_conversion_narrow.rs`) | `LayerRaster::filled` (none) | the byte adjustment layer's extent; `.rz` / PSD via `to_contiguous` / `runs` |
  | `precision_geometry.rs:151/154` | Unseeded | the installed layer; `image_equal` runs |
  | `retained_source_render.rs:373` | Unseeded | `render16`'s `rendered` layer, `render8`'s quantize rows |
  | `develop_inspect.rs:89` | Unseeded | the host handle (lend binds its own view) |
  | `doc_export.rs:1316` | Seeded | sharpen `:1329`, `convert16` `:1097` (13B-3e) |

- **The narrowing's other seeds stay** (`precision_conversion_narrow.rs:233`
  base raster, `:400` source pixels, `:127` `unit_pixels`), so
  `no_view_filled_planes`' narrowing half pins exactly the base's `w·h·4`.
- **The example's `layer_retained_bytes`** is `retained_bytes()`, which
  also counts the image's (shared) profile (9121 bytes, sRGB, at HEAD and
  tree alike); the spec's figures 1227776 / 3267328 (HEAD 192003072 /
  96000768) are the PLANE alone, printed as `layer_plane_bytes`
  (`precision_pixels` + `tile_structure` in the report). The test oracle
  asserts both: the plane by hand and `retained_bytes()` = plane + the
  report's `profile` bytes.

### 13B-3e-1 decisions

- **D1 — 13B-3e SPLIT into four runs** (plan table): 13B-3e-1 the export /
  PSD / float writers (this run), 13B-3e-2 the live filter and the retained
  source, 13B-3e-3 raw develop, the scalar filter and the other whole
  readers (may split again), 13B-3e-4 the FFI lend, `contiguous_rgba16/32`
  removed, the reader-side decoders, the reopened empty layer, the
  narrowing's seeds, `OutputView::Seeded` removed and the above-cap f32
  planes tiled. The export chain goes first because its readers are all
  sequential or one rect, and because every one of its Seeded sites has
  only readers inside this run.
- **D2 — `Extent`** is the byte and typed scans' ONE extremes record, so
  `bounds_from` (byte, forward) and `alpha_bounds16` (typed, TILE-BAND) share
  the arithmetic; min / max are order-independent, so the band order answers
  the forward scan's rectangle. Inside a run only its first and last visible
  pixel can move an extreme, so the typed scan includes those two
  (`position` / `rposition`) — the same rectangle for less work (the first
  cut, which included every visible pixel, measured 1.08 × HEAD warm on
  `export_layer_trimmed`; 0.93 × after).
- **D3 — the "covering the canvas" refusal is removed**: it guarded a
  sample slice shorter than the canvas, which a tile read cannot produce
  (`alpha_bounds16` walks the image's own dimensions); the storage refusal
  keeps HEAD's first sentence and position.
- **D4 — `OutputView::Seeded` stays with `#[allow(dead_code)]`**: the spec
  keeps both variants until 13B-3e-4 removes the variant and its arms
  (`precision_render.rs:789/805`, `float_document.rs:627`,
  `precision_sampling.rs:64/78`) together.
- **D5 — the PSD channel planes come from ONE walk** (`channels16`, a
  deviation from reading `samples16` once per channel): the first cut read
  the four channels through four `samples16` walks and measured the
  flattened save 1.34 × HEAD warm; a `sample` profile showed the channel
  extraction at ~2 × HEAD's slice passes (a tile run is 2 KiB, so each walk
  restarts the prefetcher 24 times per row). One walk writing four planes
  brings it to ~1.1 × of HEAD's four slice passes (the save itself
  1.08 ×, MISSED, recorded). The PackBits encode per channel is unchanged,
  so every byte is HEAD's. `plane16` keeps serving the masks. Its peak
  (carried nit 2, recorded in 13B-3e-2): `channels16` holds the four raw
  planes at once, `w·h·8` bytes (192000000 at 24 MP), where HEAD's
  `plane16` held one at a time, `w·h·2`. That is within `save_psd`'s admitted
  `canvas_bytes` (`w·h·8`, `psd_export16.rs:531`).
- **D6 — `put_raster`'s staging copies each run in `split_at` chunks** up
  to the 4096-sample boundary (the first cut indexed one pixel at a time:
  1.28 × HEAD warm; unchanged by the chunking, see the verification
  record). The poll and `append` sequence is HEAD's: one `job.check()` and
  one `append` per full 4096-sample chunk, then one for the partial tail.
- **D7 — nit 2's oracle**: the `retained_bytes()` profile term is the
  `profile` category of `rz_mixed_resources_report` over a SEPARATE 1×1
  `from_rgba16` probe on `IccProfile::srgb()`, not the report under test,
  and it is also asserted `> IccProfile::srgb().bytes().len()`: the ICC
  byte length alone is not the retained term — the `IccProfile` struct, its
  name and its three sampled TRC tables count too (9121 bytes for sRGB,
  13B-3d-3b decisions).
- **Classification** (site → view → readers):

  | Site | View | Readers |
  | --- | --- | --- |
  | `export_encoded`'s render (`doc_export.rs:1271`) | Unseeded | `export_stages`: the tone map / resize (runs / transient), the sharpen and `convert16` (runs), `encode` (runs), quantize (rows), `encode_float_tiff` (runs) |
  | `export_stages`' tone map (`:1301`) and resize (`:1320`) | Unseeded | the sharpen and `convert16` (runs), `encode` (runs), quantize (rows) |
  | `convert16`'s output (`:1117`), the sharpen's (`:1340`) | Unseeded | `encode` (runs), quantize (rows) |
  | `export_layer_encoded`'s render (`doc_export_layers.rs:312`) | Unseeded | the trim (TILE-BAND scan, one rect), `export_stages` |
  | `crop16`'s output (`:135`) | Unseeded | `export_stages` |
  | `precision_io::export16`'s render (`precision_io.rs:123`) | Unseeded | `precision_codec::save` → `encode` (runs) |
  | `bake_style16` (`precision_style_render.rs:919`) | Unseeded | `psd_export16`'s record (`samples16`) |

### 13B-3e-2 decisions

- **D1 — runs for the two sequential reads, no transient.** `source16`
  hands the coloured plane's `runs_rgba16` to `filtered`, whose
  `straight_plane` walks runs with a running index (the
  `precision_conversion::promote_runs` / `convert8_runs` shape), so each
  `job.every` lands on the sample the slice walk polled at. `color16` also
  reads rows in plane order, through a row buffer (D6).
- **D2 — the preview's row cursor over per-point `rgba16_at`.** The spec
  chose ONE `row_segments` per output row with a forward cursor (the cell
  centres rise strictly with `x`), rather than a per-point tile lookup. It
  was built and measured: `preview_after` 1.23 × HEAD warm, then 1.20 × after
  the fix pass. It was reverted (D5).
- **D3 — `render8`'s 16-bit output unseeded.** This was the retained
  source's last seed. Its readers are byte layer readers, view-free since
  13B-2c-3b (residual 0).
- **D4 — the byte `source8` transient kept.** `to_contiguous_with_job`
  polls once per tile. Walking the asset's runs would drop those
  checkpoints, so the byte chain keeps HEAD's sequence. `filtered` takes
  runs now, so the change is one line when the checkpoint cost is accepted
  (open item, 13A-latency).
- **D5 — the preview MISS revert** (spec Constraints: one fix pass, then that
  line's path reverts to HEAD's read AND the seeded producer(s) feeding it).
  `preview_after` (2000×1500, the filtered plane from the masked cache,
  divisor 2) read the tiles through D2's cursor at 1.229 × HEAD warm (load
  ~11–14). After the fix pass it was 1.197 × (load ~24–31). Diagnostic
  probes (recorded only, 21 previews per process, load ~6–16) found that
  neither a run-outer / point-inner restructure (1.87 vs 1.54 ms) nor a
  burst copy of the row span (2.2–2.3 ms) recovered it. A sampled output
  row spans 8 tile runs of 2 KiB where HEAD read one 16 KiB contiguous row.
  So `live_filter_preview.rs:160` reads `contiguous_rgba16()` again, and its
  feeding producer `live_filter_evaluate.rs:423` (the filtered plane, the
  typed masked-cache entry) is `from_rgba16` Seeded again, so the first
  preview after an edit is a cache hit as at HEAD. **Residual**: every
  typed live-filter edit seeds its filtered plane (`typed/live_filter` typed
  edit lines `views_held` 48000000 = `w·h·8` at 6 MP; HEAD held 96000000
  with the coloured input's view, which this run removed). A 16-bit preview
  binds a view: the seeded plane when filtered, a per-call materialization
  of `color16`'s / the promoted master's unseeded output when unfiltered and
  converted (`preview_before_p3` 1.005 × HEAD, `views_held` 0 after the
  call). The byte-canvas edit holds 0 at 6 MP (its masked stage is evicted
  under the 128 MiB policy, so the seeded plane dies with the call). The
  spec's rule reverts both halves. Reverting the read alone would also meet
  the warm bound, since the benchmark evicts after the build, so the seed
  never reaches a measured call, and it would keep the edits view-free. The
  seed matters for the app's edit → preview loop (the first preview after
  each edit), and that is why the rule re-seeds.
- **D6 — `color16` copies each row's tile runs into ONE row buffer** (a
  `w`-sample transient, ≤ 240 KiB at 30000 px) before HEAD's per-pixel walk:
  the fix pass for `preview_before_p3` (1.071 × HEAD warm on straight runs,
  load ~11–14). A diagnostic probe (1×1 output, so only `color16` and its
  output count; load ~5–16) put the straight-runs walk at 468–488 ms against
  HEAD's 449–462. Variant A — the same loop over the warm contiguous view —
  measured 444–447, so the tile layout, not the loop code, costs. The row
  copy measured 439–446 against HEAD's 436–458: the per-pixel transform
  (a large encoder table) runs over one hot buffer, not over reads spread
  across 8 tiles. After the pass `preview_before_p3` was 1.028 × (load
  ~24–31), and 1.005 × in the final set.
- **Classification** (site → view → readers):

  | Site | View | Readers |
  | --- | --- | --- |
  | `source16`'s promoted byte master (`live_filter_evaluate.rs:396`) | Unseeded | `color16` (rows), `filtered` (runs), the preview (materializes once, D5), `render_high_depth` Shared (`reidentified` → layer) / Sample (`transform_image`, transient) |
  | `color16`'s output (`retained_source_render.rs:272`) | Unseeded | `filtered` (runs), the preview (materializes once, D5), `render_high_depth` Shared / Sample |
  | `source16`'s filtered output (`live_filter_evaluate.rs:423`) | **Seeded** (D5) | the `FilteredPixels::Typed` cache entry, the preview (whole, D5), `render_high_depth` Shared / Sample, `render8`'s quantize (rows) |
  | `render8`'s 16-bit render (`retained_source_render.rs:288`) | Unseeded (`LayerRaster`) | the byte layer readers (13B-2c-3b residual 0) |

### 13B-3e-3a decisions

- **D1 — the split.** 13B-3e-3 as planned is 10 reads + 4 seeds plus two
  interactive paths with their own warm risk. Raw develop / geometry /
  inspection (`raw_local_develop.rs:163/183/264`,
  `raw_source_geometry.rs:392/466`, `develop_inspect.rs:136`) is one
  coherent RAW pipeline whose develop preview is interactive, so it is
  **13B-3e-3b**. 13B-3e-3a takes the six installed-layer / caller-image
  readers, the scalar filter's two seeds and the direct paste.
- **D2 — tile-band versus row-major by order dependence.** An
  order-independent reader walks TILE-BAND order (each 256-row band, each
  256-column span, each row's run; the `precision_scalar_edit.rs:207-236` /
  `precision_stats.rs:96` shape, inlined, no shared helper) with a visit
  counter for `job.every`, so the checkpoints are the flat walk's: the
  auto-levels histogram, the plane read (a flat-index write), the cutout's
  nonempty scan. An order-dependent reader stays row-major over
  `runs_rgba16`: `auto_color`'s f64 sums and the WARP byte stream.
- **D3 — Pixelate reads each block row through `row_segments`**, keeping
  HEAD's block loops and every `job.check()` (the checkpoint sequence is
  the block walk's, not a flat one), with u64 sums as HEAD.
- **D4 — the warp source: ONE transient per call.** The warp is a
  random-access tap walk (13B-3c-2c-1's rule): `transient_rgba16` replaces
  HEAD's cached view in peak terms, so `warp_plan` is unchanged. It never
  touches the view cache, so a warp holds no view (`precision_layer_producers`'
  warp expectation `w·h·8` → 0). The interactive warm path — a second
  `warp_edit` on the same source plane — pays the copy on every call where
  HEAD hit its cached view: `typed/whole_readers` `warp_edit` (3000×2000)
  warm 1.00–1.01 × HEAD in all three measured sets (within the ≤ 1.05 ×
  bound; the copy is small next to the warp's sampling), so the transient
  stays and the Constraints' revert did not trigger.
- **D5 — the scalar filter's gray image Unseeded**
  (`precision_scalar_filter.rs:53/156`). Its readers after this run are
  view-free: `pixel(i)` (tiles) in the guide / neighborhood / legacy-blur /
  relief closures and the output loop, Pixelate's rows (D3) and
  `AutoLevels::derive` (tile-band + runs). `typed/whole_readers`
  `scalar_pixelate` / `scalar_auto_tone` hold 0 views at HEAD already (the
  seed dies with the call), and are 0.97 × / 1.00 × HEAD warm in the final
  set.
- **D6 — the direct payload paste closed without code.** A payload pasted
  in memory (`clipboard_paste`'s no-convert arm, `from_clipboard`) installs
  the payload's unseeded plane; its whole readers were this run's six, now
  on tiles (`paste/read_plane`, `paste/from_clipboard_pixelate` hold 0
  views). The open item's `precision_clipboard.rs:492` example was stale:
  it is the float Cut's storage check, a rect writer since 13B-3d-1.
- **D7 — the fix pass** (spec Constraints: a MISSED line gets one fix
  pass). The first cut MISSED `selected_auto_color` (1.086 × HEAD warm) and
  `refinement_cutout` (1.611 ×): HEAD's warm call reads the cached
  contiguous view, the tree walked tiles with a per-sample `job.every`
  branch (and, in the cutout, a bounds-checked matte index). The pass
  hoists `job.every` to each run (one `check` per multiple of 4096 in the
  run's visit range — the flat walk's checkpoints; a cancel still returns
  before any answer) in both auto-levels passes and the cutout, and zips
  the cutout's run with its matte span: `selected_auto_color` 1.006 ×,
  `refinement_cutout` 0.55 ×. In that re-measurement `warp_save` read 1.060
  × (it was 1.027 × on the first cut, noise at load ~26–34), so it took its
  own pass: the WARP encode writes each run's codes into one `resize`d span
  instead of one `extend_from_slice` per code (same bytes), 0.91 × in the
  final set. The plane read and Pixelate were not touched by the pass.
  On a contiguous (untiled) plane `runs_rgba16` yields one run per row, so
  the hoisted checks can space up to 4096 + w − 1 samples apart (the count
  unchanged).
- **Classification** (site → view → readers):

  | Site | View | Readers |
  | --- | --- | --- |
  | the scalar filter's gray image (`precision_scalar_filter.rs:53`) | Unseeded | `pixel(i)` in the filter closures and output loop, `SelectedFilter::plane` (Pixelate rows) |
  | the scalar Auto derivation's gray image (`precision_scalar_filter.rs:156`) | Unseeded | `PrecisionDocument::from_image`, `AutoLevels::derive` (tile-band + runs) |

### 13B-3e-3b decisions

- **D1 — the geometry source bind is deferred to 13B-3e-4.** A transient at
  `raw_source_geometry.rs:392` (the tap walk's 13B-2c-3b rule) would sit
  beside the host-intake seed that feeds it — `rz_precision_doc_from_rgba16`
  (`ffi_precision_input.rs:76`) / `host_pixels.rs:236` build the base the
  app develops, `RawDevelopmentRecipe.swift:428-438` /
  `RawLocalEvaluation.swift:141-143` pass it straight to the geometry — so
  the call would hold tiles + that seed + the transient, `w·h·8` of peak
  above HEAD: the `export_stages` size-split hazard (13B-3c-2c-1). So `:392`
  keeps its view bind until 13B-3e-4 unseeds the host intakes, when ONE
  transient replaces their seed. Per-tap `rgba16_at` reads are declined:
  13B-2c-3b decided tap walks take one transient, never per-tap tile reads
  (13C / 13A-latency), and the app's geometry is the filtered lens arm.
- **D2 — `raw_local_develop.rs:264` stays Seeded.** Its whole reader
  `raw_source_geometry.rs:392` remains (D1): a develop → lens call (the
  app's recipe order) binds the develop output whole, so unseeding it would
  make every such call materialize a view HEAD never built (the 13B-3c-2c-1
  Seeded-while-a-whole-reader-remains rule, row 5511). It moves with `:392`
  in 13B-3e-4.
- **D3 — the develop's walks and its fix pass.** Row-major runs (the flat
  `i` indexes `accumulated` and the mask), `storage()` gates at HEAD's two
  positions — the variant's BEFORE the early `None`, pinned by
  `refuse/float_variant` (a disabled float variant, no vignette). The first
  cut kept a per-sample `job.every(i)` in every pass and MISSED
  `develop_local` (24 MP warm 263.6 → 277.5 ms, 1.052 ×, load ~5–9). The
  named fix pass hoists `job.every(i)` to each run through `EVERY_INTERVAL`
  in passes 1 and 3 — exact, since nothing fallible sits between two
  checks — and zips pass 3's run with its `accumulated` span; pass 2 keeps
  its per-sample checks (`effective.at(i)?` is fallible). `develop_local`
  0.83 ×, `develop_vignette` 0.97–0.99 × after it.
- **D4 — the preview MISS and its revert.** The tile band walk (clipping 0:
  ONE `row_segments(cy, …)` with a column cursor; clipping ≠ 0: every row
  of the band through `row_segments`, each run split at the cell
  boundaries with one division per run, the markers ORed into per-column
  `zero` / `maximum` and the centre captured on row `cy`, three
  `width`-long `reserved_vec`s ≤ 16000 B) was byte-identical with HEAD's
  checkpoints and held 0 views, but MISSED the bound: first cut
  `preview_fit` 8.80 → 12.10 ms (1.375 ×), `preview_fit_clipping` 62.49 →
  84.95 ms (1.359 ×), `preview_detail` 9.57 → 11.95 ms (1.249 ×), load
  ~5–9; after the named fix pass (`job.every(visited)` hoisted per run)
  8.86 → 11.86 (1.338 ×), 62.65 → 66.21 (1.057 ×), 9.87 → 10.37 (1.051 ×),
  load ~10–24. So, per the Constraints, `develop_preview` reverted to
  HEAD's body verbatim (`contiguous_rgba16()` at `:135-137`) and
  `raw_source_geometry.rs:466` stayed `from_rgba16` (Seeded): the app feeds
  the geometry output to the preview through `layerSource(0)`
  (`RawDevelopPreviewSession.swift:246-264`), so unseeding it would make
  every such preview materialize the view. After the revert all seven
  `typed/raw_develop` lines are 0.83–1.015 ×. The clipping-0 walk's cost is
  the per-column read through a tile row (cf. the 13B-3e-2 preview: a
  sampled row spans many tile runs where HEAD read one contiguous row);
  the preview read moves with `contiguous_rgba16`'s removal in 13B-3e-4.
- **D5 — the 9f target sets are FIXED from the 9G source**, not from the
  app's reply: admitted `red` / `alpha` (`PaintTarget.swift:120`,
  `AgentServer.swift:1469-1472` → `AgentServer+PrecisionScalarEditing.swift:64-71`),
  refused `mask` (`OperationScope.swift:39`; the 9f fixture is an unmasked
  PNG layer) and `channel:missing` (`AgentServer+ChannelTargets.swift:31-35`).
- **D6 — `EVERY_INTERVAL`** (carried nit 1): the one poll interval of
  `JobContext::every`, named so the hoisted loops step by the same
  constant; nit 2's sentence is added to 13B-3e-3a D7.
- **Classification** (site → view → readers):

  | Site | View | Readers |
  | --- | --- | --- |
  | the develop output (`raw_local_develop.rs:264`) | Seeded (D2) | `raw_source_geometry.rs:392` (whole bind, 13B-3e-4), `develop_inspect.rs:136` (whole bind, the D4 revert), the compositor / thumbnails (rows) |
  | the geometry output (`raw_source_geometry.rs:466`) | Seeded (D4) | `develop_inspect.rs:136` via `layerSource(0)` (whole bind, the D4 revert), a following `raw_source_geometry.rs:392`, the compositor (rows) |

### 13B-3e-4a decisions

- **D1 — 13B-3e-4 is SPLIT into 4a / 4b / 4c** (plan table). 4a (this run):
  the host intakes and the develop output unseeded, the geometry source on
  ONE transient, the lend private — exit `rg -c` 7 → 4, seeded sites
  20 → 17, every record line identical to `6b97c48`, every bounded peak
  line tree ≤ HEAD. 4b: the reader-side decoders (each checked against its
  whole readers), the reopened empty layer filled, the narrowing's seeds,
  `OutputView::Seeded` and its arms removed — exit seeded sites 17 → 2
  (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:470`). 4c: the
  two kept preview reads under the ≤ 1.05 × HEAD rule or a narrow
  crate-private view-backed accessor kept as an exception (owner
  13A-latency), `contiguous_rgba16/32` removed, f32 planes above
  `CONTIGUOUS_CACHE_BYTES` tiled — exit `rg -c` 0.
- **D2 — the three unseeds** (one-line flips). Each whole reader after 4a
  is named below; the rows / runs readers (the compositor, thumbnails,
  `encode`, `develop`) bind nothing. App callers:
  `RasterDocument.fromRawRGBA16` (`RasterCore.swift:4813`;
  `RawDevelopmentRecipe.swift:428`, `NewDocument.swift:254`,
  `HostSourceWork.swift:90`), `fromHostPixels` (`HostStillDecoder.swift:61`,
  `HostVideoDecoder.swift:162`), `developLocals` →
  `applyingRawSourceGeometry` (`RawDevelopmentRecipe.swift:431-437`,
  `RawLocalEvaluation.swift:131-143`), previews via `layerSource(0)`
  (`RawDevelopPreviewSession.swift:246-264`).
- **D3 — the geometry source is ONE transient.** It replaces the seed that
  is now absent (13B-3e-3b D1; 13B-3e-3a D4's peak rule), so
  `raw_source_geometry_plan` / `plan.scratch` stay HEAD's; the gate keeps
  HEAD's refusal sentence and position; an identity request copies
  nothing. Per-tap `rgba16_at` reads stay declined (13B-2c-3b). A warm
  geometry call on ONE plane (the `typed/raw_develop` geometry lines) now
  copies that plane per call where HEAD re-read the seed — the app never
  re-runs geometry on the same plane, so a miss there is recorded on the
  transient-warm row (13A-latency), not reverted.
- **D4 — the lend pins a PRIVATE copy** (`Plane::lent`). It no longer
  reads or writes the view cache; an allocation failure answers `None` (the
  FFI's NULL) instead of a caught panic. On an image built by one of the 17
  still-seeded constructor sites, while its seed is cached, the pin is a
  second `w·h·bpp` beside the evictable seed — the measured
  `lend/seeded_u16` line, closing as 4b / 4c drop the seeds. The lend is on
  no app path (`withRGBA16` / `withRGBA32` have no caller in
  `app/Sources`); probes under `scripts/` read samples through it, and
  `scripts/native-paint-probe/geometrygestures.swift` used its pointer as
  an allocation-identity oracle — moved onto the storage identity (D6).
- **D5 — the bounds were FIXED before measuring** (spec Constraints): the
  peak gate tree `peak` / `held` ≤ HEAD on every line except
  `lend/seeded_u16` (tree ≤ HEAD + `w·h·8` + 64 on both: the lent buffer
  plus its fresh `Arc` header); `sha256` identical and `views_held`
  exactly as declared on every example line; `ms_warm` ≤ 1.05 × HEAD
  (median of 3 alternated samples) on `intake_rgba16`,
  `intake_host_pixels`, `pipeline_develop_lens` and the seven
  `typed/raw_develop` lines; `pipeline_exif6` (its geometry copies through
  ONE transient the plane HEAD's constructor adopted as the seed — the
  13B-3e-3b `geometry_exif6` `ms_first` 159.22 vs `ms_warm` 142.50 gap) and
  `pipeline_preview_fit` (the first preview of a fresh plane materializes
  what HEAD's constructor adopted) recorded with no bound.
- **D6 — the storage identity is the plane key** (amendment 1, the user's
  decision on the first 9h run: a real storage identity, D4 kept). The
  value is the tile index allocation's address, or a contiguous plane's
  buffer address. It is already the core's "same storage" answer
  (`image_equal`'s shortcut, `precision_geometry.rs:211/222`, and the
  dedup, `precision_color.rs:114/343`); it is exactly what HEAD's lend
  pointer compared, because HEAD's view was keyed by
  `PlaneKey::Rgba16(weak_index())`; the integer Move's plan is
  `WorkKind::Keep` (`precision_geometry_canvas.rs:178-180`), so the moved
  layer keeps its index; it reads no view, so it survives 4c
  (`contiguous_rgba16/32` removed, f32 planes tiled); and equality implies
  every tile is shared — the no-copy guarantee. The lend's pointer is
  per-handle and never an identity.
- **Classification** (site → view → whole readers after 4a):

  | Site | View | Readers |
  | --- | --- | --- |
  | the host intake `ffi_precision_input.rs:76` (`rz_precision_doc_from_rgba16`) | Unseeded | `raw_source_geometry` (ONE transient, this run), `develop_inspect.rs:136` (view: the FIRST preview of a fresh plane materializes it, later calls hit the cache — 4c), `live_filter_preview.rs:164` through `color16`'s same-profile share (`retained_source_render.rs:242-244`) when the image becomes a source master (view, first call — 4c), the lend (private); rows / runs: the compositor, thumbnails, `encode`, `develop` |
  | the host intake `host_pixels.rs:236` (`rz_precision_doc_from_host_pixels`) | Unseeded | the same readers |
  | the develop output `raw_local_develop.rs:302` | Unseeded | the same readers (the app passes it to `applyingRawSourceGeometry`, then previews the result) |

### 13B-3e-4b decisions

- **D1 — the decoders get their OWN constructors** (`from_rgba16_decoded`
  / `from_rgba32_extended_decoded`) rather than a flip to the existing
  unseeded pair: a decoder is the one place a bitwise-uniform plane arrives
  as an adopted `Vec` (a reopened `add_layer_at` layer, a pixel-less PSD
  record, a flat RZCB payload), and only there is the uniform scan worth
  its cost. Every other unseeded producer (the paste conversion, the
  promotion, the Copy / Cut payload) keeps `Tiling::Unseeded` and its
  exact tiles. The decoded pair runs the unseeded statements verbatim, so
  refusals, checkpoints and reports cannot move.
- **D2 — `uniform_tiles` is bitwise and bounded.** `Sample::same_bits` is
  the tile store's only equality (f32 by bits: `-0.0` among `0.0` stays
  dense, as `with_rect` would allocate it); `tiles_of`'s
  `CONTIGUOUS_CACHE_BYTES` bound applies (above it the plane stays
  contiguous, as today); the scan stops at the first mismatch, so a
  non-uniform plane pays one sample comparison in the common case and a
  plane differing only in its LAST sample one full pass (that case is
  pinned dense by `reopened_uniform_planes_are_filled`). A filled plane is structural
  sharing only; `with_rect` / `rebuilt_tiles` / `same_samples` /
  `record_resources` already serve filled planes (13B-3d-3b).
  **Admission and plans are unchanged**: `reopen_retained_bytes` and the
  PSD `budget` still charge the dense plane (tightening → 13B-8); a
  resource report shows the smaller `precision_pixels` of a uniform plane
  and nothing else changes.
- **D3 — the narrowing's byte planes are unseeded.** The base raster
  (`:233`), the WARP original (`:400`) and every 1×1 placeholder (`:127`)
  have no whole reader: the byte compositor, the regional tree, the save and
  the thumbnail read rows / runs / `sample_at`, and the byte
  `contiguous_image` residual is 0 since 13B-2c-3b. `LayerRaster::unseeded`
  keeps `new`'s contiguous fallback.
- **D4 — `OutputView::Seeded` is removed with the two seeded render
  constructors** (`from_float_samples`, `from_rendered_float`: no caller
  once the arms go). `OutputView` keeps ONE variant so no signature moves
  in this run; removing the type and its `view` parameters (8 functions,
  ~35 call sites in 18 files) is a mechanical 13B-3e-4c item (Open items).
  The seeded `from_rgba16` / `from_rgba32` / `from_rgba32_extended` stay:
  test-built images and the two kept preview seeds use them.
- **D5 — the gates were FIXED before measuring** (spec Constraints):
  `sha256`, `checkpoints` and `reports` identical to HEAD on every record
  and example line; `views_held` exactly as declared; tree `peak` ≤ HEAD
  and tree `held` ≤ HEAD on every peak line; `ms_warm` (median of 3
  alternated HEAD / tree samples) ≤ 1.05 × HEAD on all 11 `typed/decoders`
  lines — every one is interactive (open or reopen → first render /
  preview, paste, convert → render); `ms_first` recorded with no bound. A
  MISS (timing after ONE fix pass, or any peak / held line) would have
  returned every site the line decodes through to HEAD's constructor and
  re-declared every dependent expectation with a comment citing it. No
  line MISSED, so nothing was reverted.
- **Reader classification** (site → constructor after this run → whole
  readers, i.e. the reads that still bind a view or otherwise read the
  whole image; every other reader reads rows, runs, a rect or ONE
  transient):

  | Site / its callers | Tree | Whole readers |
  | --- | --- | --- |
  | `precision_png.rs:153` (`decode_native_png`): typed `.rz` layers (`precision_native_read.rs:1011`) and every u16 source-asset master, byte or typed `.rz` (`native_sources_assets.rs:355`) | `from_rgba16_decoded` | Layers: none. Masters: `develop_inspect.rs:136` (`sourceMaster16` → Develop "before", `inspect_raw_development`, the migration review) and `live_filter_preview.rs:164` (the unfiltered same-profile 16-bit preview, through `source16` → `color16`'s share). Both materialize on the first call (13B-3e-4c). |
  | `precision_codec.rs:495` (`decode_with_context` ← `precision_io.rs:94` `open_preserving`) | `from_rgba16_decoded` | An opened document: none. A placed 16-bit master (`ImageSourceFile.swift:71` → `source_asset.rs:178`): `live_filter_preview.rs:164`. |
  | `psd_import16.rs:281` (`raster_layer`, incl. the canvas-sized pixel-less record), `:427` (the composite) | `from_rgba16_decoded` | none |
  | `float_io.rs:158`, `float_native.rs:142` (← `precision_native_read.rs:993`) | `from_rgba32_extended_decoded` | none |
  | `warp_native.rs:534` (`attach_typed`) | `from_rgba16_decoded` | none (`precision_warp.rs:85` reads a transient, `:217` reads runs); the `same_samples` dedup at `:560` still shares |
  | `precision_clipboard_codec.rs:313` / `:545` / `:568` (`ffi_clipboard` ← `ClipboardPayload.swift:75/96`) | `from_rgba16_decoded` / `from_rgba32_extended_decoded` | none (quantize rows, `encode` / `converted` runs; paste installs it) |
  | `precision_conversion_narrow.rs:127` / `:233` / `:400` | `LayerRaster::unseeded` | none (the byte `contiguous_image` residual is 0 since 13B-2c-3b) |

### 13B-3e-4c-1 decisions

- **D1 — 13B-3e-4c is SPLIT into 4c-1 / 4c-2.** 4c-1 is the accessor
  removal and the test migration (421 calls, 81 files); 4c-2 owns
  `OutputView`'s removal, `tile()`'s f32 seed, f32 tiling above
  `CONTIGUOUS_CACHE_BYTES` and carried nit 5. Doing both in one run would
  have mixed a mechanical signature sweep with a representation change.
- **D2 — the previews take the request's SECOND arm**: a narrow
  crate-private view-backed accessor, kept as an exception owned by
  13A-latency. No new tile-read attempt: 13B-3e-2 (the live-filter preview,
  1.23 × / 1.20 × HEAD) and 13B-3e-3b (the develop preview, 1.34 × / 1.06
  × / 1.05 × HEAD) each MISSED after a fix pass, so the tile row read
  belongs to 13A-latency. `preview_view_rgba16` is HEAD's
  `contiguous_rgba16` body verbatim, `pub(crate)`, u16 only, so every
  answer, refusal sentence and position, checkpoint, report log, `.rz` byte
  and `views_held` is HEAD's.
- **D3 — tests read owned copies, per-sample reads and the storage
  identity.** A pointer taken from a `to_rgba16/32` copy is never an
  identity, so the five view-ADDRESS oracles read
  `rz_precision_image_storage_identity`; a per-sample read never takes a
  copy per sample. The view-cache contract the subject tests pinned through
  the removed accessor is pinned through the one remaining reader
  (`preview_view_exception`).
- **D4 — Open items dispositions**: CLOSED in 4c-1 — the "residual after
  13B-3e-4a" row and the "Typed `.rz` / PSD / export reads off the views;
  residual 72 → 0" row. Owner → 13A-latency only (the read is now
  `preview_view_rgba16`): the 13B-3e-2 and 13B-3e-3b preview-MISS rows and
  the "Seeded sites … with another whole reader" row. Owner →
  13B-3e-4c-2: `OutputView`'s removal, and f32 above-bound tiling (since
  4c-1 no f32 whole-view reader exists). The seeded lend's second copy:
  13B-3e-4c-2 (the f32 test seeds) plus 13A-latency (the two kept preview
  seeds). The first preview of a fresh unseeded plane: 13A-latency. New
  row: carried nit 5 → 13B-3e-4c-2.

### 13B-3e-4c-2a decisions

- **D1 — 13B-3e-4c-2 is SPLIT into 4c-2a / 4c-2b.** 4c-2a is the
  mechanical sweep: `OutputView` and its nine parameters, the f32 seed, the
  f32 view type, the alias and nits 1 / 2 / 5 — no production path changes
  (every output was already unseeded). 4c-2b is a representation change for
  planes above 1 GiB: once such an f32 plane is tiled, its tiling transient
  and the `transient_rgba32` / `Plane::lent` / `with_rect` copies that today
  BORROW the contiguous plane would each copy > 1 GiB, which needs its own
  admission and tracked-allocator peak analysis against HEAD and must not
  add an uncharged full copy. Mixing the two would have hidden the second
  behind the first.
- **D2 — `from_rgba32_extended_unseeded` is DELETED, not kept as an
  alias.** Once no f32 constructor seeds, it and `from_rgba32_extended` run
  the same `adopted_rgba32_extended` statements with the same tiling, so
  the alias names a difference that no longer exists; its two callers call
  `from_rgba32_extended`. This also closes carried nit 5 (the build file
  616 → 594 lines).
- **D3 — the seeded-site `rg` is redefined** as `rg -n
  'PrecisionImage::from_rgba16\(' core/src | grep -v
  '^core/src/precision_image'`: since no f32 constructor seeds, the f32
  constructors are no longer seeded sites. The count is unchanged (2 lines,
  the two kept preview seeds, owner 13A-latency).

### 13B-3e-4c-2b decisions

- **D1 — closed as a recorded residual; no tiling code lands.** A user
  decision after the implementer's STOP: the spec assumed a float document
  can hold an f32 raster above `CONTIGUOUS_CACHE_BYTES`, and it cannot
  (D2).
- **D2 — the evidence and its reach.**
  - Measured: the open. An 8200×8193 1-channel deflated all-0.0 float TIFF
    (P = 1074921600 B > 2^30) is refused by `open_preserving` with
    "precision operation needs 2151953720 bytes including retained inputs;
    working limit is 2147483648 bytes", identically at HEAD `d1f7601` and
    with the tiling code (verification record below).
  - Code-cited: every document goes through `build` → `native_preflight`
    (`precision_document.rs:567`). The float `finish_plan`
    (`precision_native_plan.rs:402-405`) charges `reopen_retained` ≥ D +
    1 MiB plus `decode_scratch_bytes` ≥ D + 16·n + 1 MiB, where D = Σ w·h·16
    over the rasters, so D < 2^30 − 1 MiB. Every document raster is
    therefore within `tiles_of`'s bound and already tiled. The other paths
    refuse too:
    - `add_layer_at` refuses before allocating
      (`preflight_raster_addition`, `precision_native_plan.rs:94-114`;
      `precision_insert.rs:129`);
    - RZCB refuses at `precision_clipboard_codec.rs:519` (`2·samples` >
      2^31);
    - the v40 reader builds a document.
  - What still reaches above the bound: direct `from_rgba32` /
    `from_rgba32_extended` (tests, examples). Also, code-derived and NOT
    measured, a transient f32 render over an enlarged canvas. `CanvasSize`
    keeps layer extents (`precision_geometry_canvas.rs:344-352`). Worked
    example: an 8000×8000 float layer on an 8200×8193 canvas: 1024000000
    retained + 1075970176 scratch ≤ 2147483648. Both stay contiguous by
    HEAD's rule, and the render output adopts its accumulator with no copy.
    Neither is uniform-built, so the backed-out code would not have tiled
    either.
- **D3 — backed out:**
  - `within_cache_bound`;
  - the unbounded `uniform_tiles`;
  - `rebuilt_tiles`' refusal;
  - `Taps32` / `transient_rgba32 → Taps32` / `permuted32`;
  - `filled_plane` at any size;
  - the probe suite (archived in receipts).

  Reason: it has no reachable production effect, and its probe cannot
  build its fixture.
- **D4 — the three ignored fallback tests keep HEAD's text and are not
  re-run:** no code under them changed.
- **D5 — the spec's D7, re-verified at HEAD, is a new 13B-8 item** (open
  items, below: the typed geometry plan charges no input transient).

## Verification record (13B-1)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check` — clean. `make lint` — clean.
- `cd core && cargo test --release --test tile_store` — 19 passed, 1 ignored
  (the bench): geometry, refusals, round trips at all five sample types,
  `read_rect`, `with_rect` sharing (8 shared, 16 unique tiles =
  16·262144 bytes), edges, bitwise latch (signed zero allocates with 8 shared;
  NaN rewrite `Ok(None)`), `filled` ≤ 4 tiles, the 8-dab history (416 unique /
  3456 referenced), the four parked cancels (k = 193, 193, 3, 7) and the four
  `visited + 2` / stage-5 counts.
- `make test` twice — both green: 2718 passed, 0 failed, 22 ignored each.
- `make cancel-bench` (serial, once; load before `10.27 16.58 15.89`), the
  tile_store lines:
  - `to_contiguous` (6000×4000 RGBA8, 384 tiles): 0.031 ms cancel-to-return;
    entry checkpoint 2, k 50, issued at 52, 53 at return; c_tile 18.176 µs;
    bound T_f + c_tile = 1.018 ms; **PASS**; load `7.58 15.07 15.36`.
  - `from_contiguous`: 0.028 ms; entry 2, k 50, issued at 52, 53 at return;
    c_tile 26.892 µs; bound 1.027 ms; **PASS**; load `7.58 15.07 15.36`.
  - Every other suite's line in the run: 26 PASS, 0 MISS.
- `cd core && uptime && cargo run --release --example phase13b_tile_sharing`
  (load `7.74 14.85 15.28`):

  | Size | op | unique_tiles | referenced_tiles | unique_bytes | referenced_bytes | shared_bytes_saved | index_bytes | contiguous_bytes | ms |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | 6000×4000 | from_contiguous | 384 | 384 | 96000000 | 96000000 | 0 | 3072 | 96000000 | 12.56 |
  | 6000×4000 | to_contiguous | 384 | 384 | 96000000 | 96000000 | 0 | 3072 | 96000000 | 9.84 |
  | 6000×4000 | filled | 4 | 384 | 612352 | 96000000 | 95387648 | 3072 | 96000000 | 0.07 |
  | 6000×4000 | dab_sequence | 416 | 3456 | 104388608 | 864000000 | 759611392 | 27648 | 864000000 | 0.93 |
  | 8000×6000 | from_contiguous | 768 | 768 | 192000000 | 192000000 | 0 | 6144 | 192000000 | 24.06 |
  | 8000×6000 | to_contiguous | 768 | 768 | 192000000 | 192000000 | 0 | 6144 | 192000000 | 17.40 |
  | 8000×6000 | filled | 4 | 768 | 471040 | 192000000 | 191528960 | 6144 | 192000000 | 0.06 |
  | 8000×6000 | dab_sequence | 800 | 6912 | 200388608 | 1728000000 | 1527611392 | 55296 | 1728000000 | 0.76 |

  The 24 MP dab history (base + 8 states) retains 104.4 MB of tiles where
  nine contiguous buffers would retain 864 MB.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED.
- `verify_phase12e2b_placement.py` (receipts `13b/13b-1/12e2b-run2`) — ALL
  GATES PASS, P11 parked count 0 flat through 200 edits. The first run
  (`13b/13b-1/12e2b`) failed P11 (start 2, at-20 0) under the carried nit's
  literal "own count" rule: `sample` splits one thread into several entries by
  dispatch queue, and keeping the LAST entry's count as the thread's own count
  judged two running animations (163/612 and 151/612 samples) parked. The
  rule now sums a thread's entries; re-read over 12E-2b's receipts it gives
  the docstring's 62 and 2, and over 13A-7's and this run's samples 0.
- `verify_phase12h_conversion.py` (`13b/13b-1/12h`) — pass.
  `verify_phase12g_clipboard.py` (`13b/13b-1/12g`) — pass.
- `verify_phase9h_ui.py --require-current --report 13b/13b-1/9h.json` —
  every case PASS except `current-layer-commands-ui` ("the owned editor never
  became the real key window"); re-run alone with
  `RZ_PAINT_UI_CASES=current-layer-commands-ui` (`9h-current-layer-rerun.json`)
  failed the same way while `ioreg` reported `CGSSessionScreenIsLocked = true`:
  the known lock-state failure, not a code failure (no app code changed).
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards;
  `scripts/**/__pycache__` removed.

## Verification record (13B-2a)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check` — clean. `make lint` — clean.
- `cd core && cargo test --release --test layer_raster` — 11 passed:
  dimensions; `contiguous_image` twice `Arc::ptr_eq` with the source bytes;
  `to_contiguous` sample-for-sample and independent of the store; `read_rect`
  against hand-sliced oracles (interior, each edge, full plane, 1×1 plane,
  1×1 rect, a 530×330 rect across the 256-px seams of the 6000×4000 phase-4
  corpus and its bottom-right corner, zero-area, `x+w > width`,
  `y+h > height`, both u32-wrap forms `None`); `ptr_eq` (clone true, equal
  bytes false, `shared` keeps the `Arc`); `weak_count` 0 fresh, 0 before a
  styled composite, 1 while the style's cache holds the key, 0 after the
  document holding the style drops; `from_pixels` → `with_layer_name` shares,
  `with_layer_pixels` does not; two groups' rasters are not `ptr_eq`.
- `make test` twice — both green: **2729 passed, 0 failed, 22 ignored** each
  (the 2718 baseline + the 11 new). Load before run 1 `10.94 13.32 11.77`
  (5:39 wall, cold build); run 2 before `7.38 14.59 17.18`, after
  `12.72 14.90 16.97` (2:10 wall). `tests/resources.rs` (the `layer_pixels`
  2n / 3n / allocation-count-2 figures) is unedited and green; the PSD
  goldens are untouched (`git status` clean under `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `12.39 14.76 16.90`, after
  `8.96 13.64 16.40`): 20 tests passed; every one of the 26 bound lines
  **PASS**, 0 MISS (tile_store: `to_contiguous` 0.029 ms, `from_contiguous`
  0.026 ms).
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load
  `30.69 20.65 18.73`).
- `cd core && uptime && cargo run --release --example phase13b_tile_sharing`
  (load before `38.04 32.40 24.81`, after `49.41 35.03 25.87`) — the 13B-1
  figures unchanged: 24 MP `dab_sequence` 416 unique / 3456 referenced tiles
  (104388608 / 864000000 bytes), 48 MP 800 / 6912; `filled` 4 tiles at both
  sizes.
- Drives against the built app (receipts `13b/13b-2a/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `43.75 34.29 25.71`) — ALL
  GATES PASS, P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `36.71 33.49 25.93`) — every
  gate PASS; `verify_phase12g_clipboard.py` (`12g`, load `35.62 33.32 25.91`)
  — every gate PASS; `verify_phase9h_ui.py --require-current --report
  13b/13b-2a/9h.json` (load `16.07 25.67 24.55`) — 55 PASS, 1 FAIL:
  `current-layer-commands-ui` ("the owned editor never became the real key
  window") with `ioreg` reporting `CGSSessionScreenIsLocked = true` — the
  known lock-state failure recorded by 13B-1, not a code failure (no app
  code changed).
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards;
  `scripts/**/__pycache__` removed.
- `rg -n 'Arc::(as_ptr|downgrade|make_mut|weak_count|ptr_eq)\(&(mut )?[a-z_.\[\]0-9]*\.pixels\)' core/src core/tests`
  — empty. `rg -n 'Arc<RgbaImage>|Weak<RgbaImage>'` over `doc.rs`,
  `retained_source.rs`, `warp.rs`, `style_cache.rs`, `doc_group_query.rs` —
  empty.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 46 = 44 calls
  + 2 comments.
- **Bind-once review** (every hit read by hand): each call is at the top of the
  function or block that owns the read, in a single statement that ends the
  borrow, or once per LAYER / per binding in a per-layer loop or closure — no
  call sits inside a per-row or per-sample loop body or closure. The four
  named hoists landed (`precision_scoped_source.rs` before
  `scoped_statistics::evaluate`, `doc_cutout_render.rs` before `for y`,
  `inpaint_source.rs` before `for y`, `doc_lock.rs` before the `for`), plus
  `doc_warp.rs::render_pixels` (before `warp_into`'s per-sample closure) and
  `ResolvedNode.pixels` (the regional per-sample `pixel`, bound at resolve).
  The compositor binds once per layer per composite (`doc.rs:346`,
  `style_composite.rs:756`, `style_knockout.rs:705`, `style_render.rs:112`).
  The scan aid's two hits are explained: `warp_native.rs:146` is the
  `for bytes in [...]` iterator expression, evaluated once per WARP binding;
  `doc_rotate.rs:124` is the per-layer comparison loop the spec names as one
  bind per layer. The other per-layer sites: `doc_preservation.rs:169/185`,
  `rzdc.rs:418`, `rzdc_capacity.rs:122`, `doc.rs:1477/1674`,
  `doc_geometry_set.rs:161`, `warp_native.rs:183/439`,
  `doc_group_query.rs:413` (`covers`, one hit test per layer).

## Verification record (13B-2b)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing — very high during the first half of the run).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_tiles` — 13 passed, 1 ignored:
  exact round trips at 1×1, 255², 256², 257² and 6000×4000 (also after
  eviction); the contiguous arm (`shared` 0 shared tiles and the adopted
  `Arc`; `rebuilt` and `with_rect(Some)` on it tile; the same write over the
  tiled raster shares all but its 2 tiles; equal writes `Ok(None)`; a leaving
  rect `Err`); `new(0×0)` / `shared(0×0)` dimensions, `read_rect`, `rebuilt(2×2)`
  tiles; a placed byte source's `layer_pixels.unique_bytes` = the base only and
  `embedded_source_pixels` = the asset once; `rz_doc_painting_layer` dabs at
  (250,200), (−100,−100), (5900,3900) and on a layer offset (−50,30) — bytes
  equal the hand oracle and `shared_tile_count` = tiles − intersected (6, 1,
  1, 4); an all-zero overlay → a fresh identity sharing every tile;
  Multiply by white over an opaque layer → NULL; `with_layer_pixels`
  one-pixel edit shares tiles − 1, equal image shares all with `!ptr_eq` and
  `contiguous_views.allocation_count` 1; a transparency-locked paint shares
  ≥ tiles − 4 with alpha frozen; the 24 MP report `layer_pixels.unique_bytes`
  = w·h·4 + 6·262144, `tile_structure` present, views ≤ the bound, eviction →
  0 and an equal re-materialized view; a 3-layer 1024² document flattened
  twice → 3 resident views; 8 concurrent readers → one view; a 300-layer
  document's views ≤ 256.
- `cargo test --release --test layer_raster` 11 passed (unedited);
  `--test resources` 7 passed, 1 ignored; `--test tile_store` 19 passed,
  1 ignored.
- `view_bound_evicts_least_recently_used` (`--ignored`, load `100.34 99.95
  75.77`) — PASS: 17 × 4096² views, cached bytes 1073741824 = the bound, the
  last hits, the first re-materializes equal.
- `fixed_24_48mp_corpus_retention` (`--ignored`, same load) — PASS with
  `layer_pixels.unique_bytes` = samples·4 + 256·256·4 (the one-pixel edit
  shares every tile but (0,0)); `referenced_bytes` still samples·4·21.
- `make test` twice — both green: **2742 passed, 0 failed, 23 ignored** (the
  2729 baseline + 13 new; the new ignored probe). Run 1 load before `93.19
  98.48 75.39`, after `25.81 70.32 67.40` (2:10 wall); run 2 after `8.73
  48.17 58.97` (2:14 wall). PSD goldens untouched (`git status` clean under
  `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `7.54 46.62 58.29`, after
  `6.45 41.42 55.84`): 20 passed, 26 PASS lines, 0 MISS (tile_store
  `to_contiguous` 0.028 ms, `from_contiguous` 0.025 ms); no new checkpoints.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load before
  `5.94 40.73 55.52`).
- `phase13b_tile_sharing`, the new document ops (before = HEAD `6989f63`
  representation, load `29.28 18.46 14.47`; after, load `16.60 27.53 43.87`):

  | Size | op | layer_pixels unique / referenced / allocations | tile_structure | contiguous_view_bytes | unique_retained | ms |
  | --- | --- | --- | --- | --- | --- | --- |
  | 6000×4000 | document_dab before | 864000000 / 864000000 / 9 | 0 | 0 | 864014899 | 354.4 |
  | 6000×4000 | document_dab after | 104388608 / 864000000 / **416** | 27648 | 96000000 | 200431659 | 82.1 |
  | 8000×6000 | document_dab before | 1728000000 / 1728000000 / 9 | 0 | 0 | 1728014899 | 693.7 |
  | 8000×6000 | document_dab after | 200388608 / 1728000000 / 800 | 55296 | 192000000 | 392459307 | 160.1 |

  `flattened` (3 × 24 MP): before `ms_cold` 508.3 / `ms_warm` 476.0 /
  `contiguous_views_after_warm` 0; after 511.1 / **454.4** (0.95 × HEAD) /
  **3**. The after `contiguous_view_bytes` is the base's seeded view (the
  dab states are `with_rect` results and seed nothing). The 13B-1 store rows
  are unchanged (416 / 3456 tiles at 24 MP, 800 / 6912 at 48 MP).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2b/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `16.62 26.71 43.10`) — ALL
  GATES PASS, P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `14.75 24.56 41.36`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, load `13.57 23.98 40.96`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-2b/9h.json`
  (load `8.11 13.78 31.53`) — 55 PASS, 1 FAIL: `current-layer-commands-ui`
  ("the owned editor never became the real key window") with `ioreg`
  reporting `CGSSessionScreenIsLocked = true` — the known lock-state failure
  recorded by 13B-1 and 13B-2a.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards;
  `scripts/**/__pycache__` removed.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 41 = 39
  calls + 2 comments (2a's 44 − the five `byte_len` sites − `covers` + one
  for `doc_transform`'s two arm-local binds).

## Verification record (13B-2c-1)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_reads` — 6 passed: `row_segments`
  over `TilePlane<[u8;4]>`, `TilePlane<u16>` and both `LayerRaster` arms at
  6000×4000, 257×257 and 1×1 (rows 0 / 255 / 256 / 3999 / 4000, spans
  [0,1) / [255,257) / [511,513) / [0,6000) / [5999,6000) / [300,300) /
  [0,7000) / [5990,7000)) — runs adjoin, one per tile column, and
  concatenate to the hand slice; `sample_at` at the seam, corner and
  out-of-range points (`u32::MAX` included) on the store and both arms;
  documents A and B (layers and clip members entirely right and left of the
  canvas, row-varying masks, a styled + Blend If raster, a knockout raster,
  an isolated styled group) flattened tiled == their contiguous twin byte for
  byte; `rz_doc_render_region` windows (200,160,120,120), (0,0,600,300),
  (599,299,1,1) at divisor 1 and (100,100,300,150) at divisor 2 == the
  cropped full-frame oracle == the twin's window; after `evict()` a
  projection and a window on B leave `contiguous_view_bytes() == 0` and
  `contiguous_views.allocation_count == 0`; `layer_bounds` on a 700×500 layer
  == (250, 200, 310, 130) with 0 views.
- `cargo test --release --test layer_raster_tiles --test layer_raster
  --test tile_store --test resources` — 13 (1 ignored) / 11 / 19 (1
  ignored) / 7 (1 ignored) passed; `a_projection_leaves_no_view_behind`
  (was `every_layer_view_stays_resident_across_a_projection`, 3 → 0).
- `make test` twice — both green: **2748 passed, 0 failed, 23 ignored** (the
  2742 baseline + 6 new). Run 1 load before `33.00 31.97 24.24`, after
  `84.68 67.93 43.12` (~4 min wall); run 2 before `78.78 66.98 42.93`, after
  `16.71 47.51 38.71` (~2 min wall). PSD goldens untouched (`git status`
  clean under `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `15.53 46.75 38.49`, after
  `11.32 41.73 37.03`): 20 passed, 26 PASS lines, 0 MISS (tile_store
  `to_contiguous` 0.029 ms, `from_contiguous` 0.028 ms).
- `make typecheck` — 0 errors (load `9.61 39.36 36.29`). `make app` — BUILD
  SUCCEEDED (load before `3.87 11.09 22.29`).
- `phase13b_tile_sharing`, interleaved BEFORE (HEAD `1fe7bfe` library with
  the new op) / AFTER, loads `10.66 41.08 36.83` / `10.04 40.45 36.63` /
  `10.04 40.45 36.63` / `10.04 40.45 36.63` (end `10.36 40.01 36.50`):

  | Run | `document_dab` 24 MP (alloc / unique bytes / ms) | `flattened` cold / warm / views | `regional_window` cold / warm / views |
  | --- | --- | --- | --- |
  | BEFORE 1 | 416 / 104388608 / 79.5 | 497.8 / 459.9 / 3 | 78.8 / 55.7 / 3 |
  | AFTER 1 | 416 / 104388608 / 80.1 | 479.7 / 462.1 / **0** | 61.3 / 59.0 / **0** |
  | BEFORE 2 | 416 / 104388608 / 80.7 | 500.7 / 466.9 / 3 | 80.6 / 56.0 / 3 |
  | AFTER 2 | 416 / 104388608 / 80.9 | 482.5 / 464.5 / **0** | 61.1 / 61.6 / **0** |

  `document_dab` at 48 MP is 800 / 200388608 in all four runs; the 13B-1
  store rows are unchanged. `flattened` warm is flat (0.99–1.00 ×), cold
  0.96 × (no view to materialize). `regional_window` cold is 0.77 × and warm
  1.06–1.10 × BEFORE (per-sample `sample_at` over the tiles instead of an
  index into a bound view) — noted for 13A-latency, not profiled here.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-1/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `42.39 19.56 24.91`) — ALL
  GATES PASS, P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `30.70 19.86 24.75`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, load `27.50 19.53 24.58`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-2c-1/9h.json`
  (load before `6.52 12.21 20.06`, after `4.37 7.25 15.50`) — 55 PASS, 1 FAIL:
  `current-layer-commands-ui` ("the owned editor never became the real key
  window") with `ioreg` reporting `CGSSessionScreenIsLocked = true` — the
  known lock-state failure recorded by 13B-1, 13B-2a and 13B-2b (run log
  copied to `9h-run.log`).
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards;
  `scripts/**/__pycache__` removed.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 32 = 31
  call lines + 1 comment (`layer_raster.rs:35`); 2b's 41 − the eight
  compositor / regional / content-box reads (`doc.rs`, `style_composite.rs`,
  `style_knockout.rs`, `style_render.rs`, `doc_cutout_render.rs`,
  `doc_regional_tree.rs`, `doc_regional_style.rs`, `doc_group_query.rs`) − the
  `doc_regional_tree.rs` field comment. `warp_native.rs:437` and
  `doc_rotate.rs:127` each carry two calls on one line.

## Verification record (13B-2c-2a)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_writers` — 14 passed (new):
  per site (`with_layer_plane`, `painting_layer_plane`, dodge/burn, bucket
  fill, Clear, the retouch-output deposit through `rz_doc_heal_to_new_layer`,
  red-eye, heal, content-aware fill) on the 6000×4000 corpus layer and on a
  700×500 layer at (−50, 30) over a 600×400 canvas, the cases a 300×300 dab,
  1×1, a rect across a 256-px seam, a rect touching the canvas corner and a
  stroke straddling the layer's edge: after `evict()` the op leaves
  `contiguous_view_bytes() == 0` (the fill: the target's `w·h·4`, the
  `inpaint_source.rs` sampling plate); the tiles holding a byte that differs
  between the source and the contiguous twin's answer lie in the tiles the
  hand-computed rects touch and `shared_tile_count == tiles − changed`; every
  byte outside the rects is the source's; every byte equals the twin's
  answer. An entirely off-canvas layer: Clear a new handle (`!ptr_eq`, every
  tile shared), the deposit its retouch layer (it writes a new canvas-sized
  layer from the caller's plate — today's answer), every other site NULL
  (bucket fill at its seed check). Empty coverage: Clear a new handle with
  every tile shared, every other site NULL. The 1×1 answers are the sites'
  own (heal / deposit / red-eye NULL: no interior, no pupil; the fill on the
  offset layer's periodic pattern NULL, as HEAD answers). Heal and the fill
  with two separated components; Clear on a transparency-locked layer NULL;
  256 live 1×1 rasters hold 1024 view bytes before and after flattening a
  document with an isolated styled group, with 0 `contiguous_views` for that
  document; heal's over-limit staircase (a 33002 × 1000 box on a 40000×1000
  canvas missing the layer) answers HEAD's sentence byte for byte; a fill
  whose windows miss its 50×50 layer answers NULL with no message. Against
  the HEAD `5bfc798` library the same suite fails 12 of 14 (every view
  assertion, the unseeded count 1020, and the fill miss's HEAD error), and
  passes the lock and heal-limit tests.
- `cargo test --release --test heal_tests --test content_aware_tests --test
  content_aware_workflow --test content_aware_photographs --test redeye_tests
  --test selection_tests --test structure_tests --test channel_tests --test
  doc_tests --test doc_retouch --test retouch_output --test lock_tests --test
  layer_raster_reads --test layer_raster_tiles --test layer_raster --test
  resources` — all green (load `16.71 9.30 14.99`).
- `make test` twice — both green: **2762 passed, 0 failed, 23 ignored** (the
  2748 baseline + 14 new). Run 1 load before `14.45 22.51 26.07`, after
  `45.22 45.98 36.61` (~4 min wall); run 2 before `39.35 44.70 36.27`, after
  `8.97 30.61 31.91` (~3 min wall). PSD goldens untouched (`git status`
  clean under `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `7.90 29.67 31.56`, after
  `6.22 26.83 30.45`): 20 passed, 26 PASS lines, 0 MISS.
- `make typecheck` — 0 errors (load `6.20 26.48 30.31`). `make app` — BUILD
  SUCCEEDED (load before `6.17 18.55 26.36`).
- `phase13b_tile_sharing`, BEFORE (HEAD `5bfc798` library with the new op;
  load `73.76 59.49 35.85`) / AFTER (load `18.00 23.75 26.59`),
  `rect_writers` on the 24 MP base:

  | Writer | BEFORE views bytes / count / alloc / ms | AFTER views bytes / count / alloc / ms |
  | --- | --- | --- |
  | heal | 96000000 / 1 / 388 / 103.8 | **0 / 0** / 388 / 72.8 |
  | dodge_burn | 96000000 / 1 / 388 / 40.4 | **0 / 0** / 388 / 10.9 |
  | clear_selection | 96000000 / 1 / 388 / 58.2 | **0 / 0** / 388 / 8.8 |
  | with_layer_plane | 96000000 / 1 / 388 / 57.6 | **0 / 0** / 388 / 40.0 |
  | content_aware_fill | 192000000 / 2 / 385 / 118.7 | **96000000 / 1** / 385 / 85.8 |

  `layer_pixels.unique_bytes` is 97048576 (385 for the fill: 96262144) in
  both runs — HEAD already shared unchanged tiles through `rebuilt`; what
  this run removes is `rebuilt`'s whole-plane seed (and the fill's one
  plate view is the `inpaint_source.rs` read, a 13B-2c-3 residual). Every
  other op's row is unchanged (`document_dab` 416 / 104388608 at 24 MP, 800 /
  200388608 at 48 MP; `flattened` / `regional_window` 0 views).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-2a/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `9.08 17.30 24.42`) — ALL
  GATES PASS, P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `6.73 15.55 23.41`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, load `6.40 15.19 23.19`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-2c-2a/9h.json`
  (load before `4.65 9.26 18.37`, after `6.53 7.08 14.63`) — 55 PASS, 1 FAIL:
  `current-layer-commands-ui` ("the owned editor never became the real key
  window") with `ioreg` reporting `CGSSessionScreenIsLocked = true` — the
  known lock-state failure (run log copied to `9h-run.log`).
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 29 = 28
  call lines + 1 comment (`layer_raster.rs:41`); 2c-1's 31 − `doc_redeye.rs`
  (the coverage pass reads the rect) − `doc_inpaint.rs` (the preview's temp
  document) − `doc_select.rs` (bucket fill grows over `sample_at`).
  `warp_native.rs:437` and `doc_rotate.rs:127` each still carry two calls on
  one line.

## Verification record (13B-2c-2b)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_compare` — 9 passed, 1 ignored
  (new): `TilePlane::same_bits` / `equals_contiguous` on u8, `[u8;4]` and
  `[f32;4]` (`-0.0` vs `0.0` false, a NaN pattern vs itself true, another
  NaN false), differing dimensions and a short slice false, 1×1, 257×257
  with the changed sample on both sides of the seam, the 6000×4000 corpus; a
  `with_rect` result vs its source false and its `rebuilt_from`-equal plane
  true. `LayerRaster::same_bytes` equal and unequal (last byte / byte (0,0))
  in every representation order — (Tiled, Tiled) sharing every tile and
  sharing all but one, (Tiled, Tiled) apart, (Tiled self, Contiguous other),
  (Contiguous self, Tiled other), (Contiguous, Contiguous) sharing the `Arc`
  and apart — plus differing dimensions, every pair checked both ways and
  with 0 view bytes after `evict()`; `equals_image` equal / last byte /
  byte (0,0) / dimensions / a longer buffer with equal samples (true), on
  both representations. Sites, each against the contiguous twin:
  `rz_doc_flip_horizontal` of a mirror-symmetric 1024×768 layer (ruler
  origin centred) NULL on both, the corpus flip equal to the twin's
  (recorded, not asserted: the NULL flip leaves 3145728 view bytes on the
  tiled document — `doc.rs:1508`'s `geom.apply` read — and 0 on the twin);
  `rz_doc_set_layer_content_preserving_mask` with the same pixels and offset
  NULL with 0 views on both, one changed byte non-NULL with 0
  `contiguous_views` on the source and exactly the one `w·h·4` `rebuilt`
  seed process-wide, equal to the twin's and to the changed bytes;
  `rz_doc_painting_layer` on a `LOCK_TRANSPARENCY` layer — a dab only over
  transparent pixels NULL on both, over opaque pixels alpha frozen, colour
  changed, bytes equal to the twin's and exactly ONE `w·h·4` view (the
  `rebuilt` seed); a `.rz` reopen of an integral-translation warp on a
  600×400 layer shares the layer and `source_pixels` (`ptr_eq`), a 3×3 bend
  does not and its original's bytes are the source's. The conversion
  (`rz_doc_convert_to_profile_with`, sRGB → Display P3, defaults, the
  6000×4000 layer): pre-cancelled → "operation cancelled" at checkpoint 1;
  parked at checkpoint 194 → "operation cancelled", NULL, parked at exactly
  194 (stage 1); uncancelled tiled output equal to the twin's byte for byte
  and `checkpoints(tiled) − checkpoints(twin) == 383`.
- `cargo test --release --test layer_raster_writers` — 15 passed (the new
  `a_bucket_fill_with_the_regions_own_colour_is_reidentified`: a uniform
  300×300 block filled with its own colour, tolerance 0, contiguous →
  non-NULL, `!ptr_eq`, every byte equal, every tile shared, 0 views).
- `cargo test --release --test layer_raster_writers --test layer_raster_tiles
  --test layer_raster --test layer_raster_reads --test tile_store --test
  resources --test geometry_noops --test persisted_warp_native --test
  persisted_warp_numeric --test persisted_warp_safety --test content_tests
  --test lock_tests --test phase8e_coverage --test phase8e_native --test
  color_tests --test heal_tests --test job_control --test job_control_park`
  — all green.
- `make test` twice — both green: **2772 passed, 0 failed, 24 ignored** (the
  2762 baseline + 10 new; the new ignored test is the bench probe). Run 1
  load before `26.83 23.11 17.23`, after `54.24 52.31 32.87` (232 s wall);
  run 2 before `54.24 52.31 32.87`, after `13.03 35.95 29.35` (151 s wall).
  PSD goldens untouched (`git status` clean under `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `11.41 34.85 29.04`, after
  `9.76 31.52 28.08`): 21 passed, 27 PASS lines, 0 MISS. The new line:
  "13B-2c-2b convert_to_profile (6000×4000 RGBA8, 384 tiles) cancel inside
  the tile walk at checkpoint 53: 0.045 ms cancel-to-return (issued at 4.6
  ms; entry checkpoint 2, k 50, issued at 52, 53 at return); c_tile 17.373
  µs; bound T_f + c_tile = 1.017 ms (T_f 1 ms); ≈0.00 tiles past the floor;
  PASS; load `10.81 34.33 28.89`" — inside the walk (4…387), not void.
- `make typecheck` — 0 errors (load `8.40 30.15 27.66`). `make app` — BUILD
  SUCCEEDED (load before `8.03 20.75 24.21`, after `8.76 19.92 23.81`).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-2b/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `9.43 19.17 23.42`) — ALL
  GATES PASS (13 PASS), P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `7.42 17.41 22.58`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, load `10.57 17.76 22.64`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-2c-2b/9h.json`
  (load before `10.28 14.85 20.09`, after `8.22 12.27 17.68`) — 56 groups +
  pass 2, 0 failures, `passed: true`, `current-layer-commands-ui` included
  (the screen was unlocked this run).
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 23 call
  lines, 0 comment hits: 2c-2a's 28 − `doc_rotate.rs` − `warp_native.rs:437`
  − `artwork_document.rs` − `doc_content.rs` − `doc_lock.rs` (the
  `layer_raster.rs` module-doc mention is gone with the compare rule).

## Verification record (13B-2c-2c)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_assets` — 8 passed (new,
  serial, `evict()` first): (a) a 300×200 source placed over a 640×480 host
  equals the source, shares its 2 tiles with the asset under a fresh
  identity, `layer_pixels` 1228800, `embedded_source_pixels` 240000 in 2
  allocations, 0 views; (b) the 6000×4000 corpus placed over a corpus host
  then a 300×300 dab at (100, 100): bytes equal the hand-patched source,
  `layer_pixels` 96000000 + the 4 touched tiles, `embedded_source_pixels`
  96000000, 380 of 384 tiles shared with the asset, 0 views; (c) the asset
  `ptr_eq` the input layer's raster, and answers the source bytes after the
  input is dropped; (d) a 257×257 Display P3 input into an sRGB host equals
  the per-pixel conversion, shares 0 tiles with the asset, `embedded`
  264196, `layer_pixels` host + 264196, 0 views; (e) scaled 0.5 then 2.0
  (nearest): `smaller` shares 0 tiles and leaves exactly the asset's one
  view (240000 bytes), `restored` equals the source and shares every tile
  with the asset again, the union report counts the asset once; (f) a
  σ 2 Gaussian live filter on a placed 257×257 source: every preview pixel
  equals the layer's `sample_at`, the preview binds no view,
  `live_filter_cached_pixels` present; (g) an 8-bit 257×257 source placed
  into a 16-bit document promotes to `v·257`; (h) `.rz` save → open → save
  byte-identical, the reopened asset equal to the source and tiled,
  `embedded_source_pixels` 240000.
- The regression set (`layer_raster_tiles`, `layer_raster`,
  `layer_raster_reads`, `layer_raster_writers`, `layer_raster_compare`,
  `tile_store`, `resources`, `retained_source_native`, `retained_sources`,
  `retained_source_work`, `linked_source_ops`, `linked_source_native`,
  `linked_source_memory`, `linked_source_capacity`, `raw_source_native`,
  `raw_source_ops`, `raw_source_memory`, `live_filter_native`,
  `live_filter_stack_native`, `live_filter_numeric`,
  `live_filter_stack_numeric`, `color_tests`, `psd_export`) — all green
  (load `25.12 42.14 31.03` → `26.62 40.10 30.92`). The corpus flip now
  records 6291456 view bytes tiled and 3145728 on the twin (nit 2).
- `make test` twice — both green: **2780 passed, 0 failed, 24 ignored**
  (2772 + 8 new). Run 1 load before `28.17 40.20 31.01`, after `54.23 57.60
  41.78` (231 s wall); run 2 before `50.69 56.81 41.60`, after `17.28 41.24
  37.98` (162 s wall). PSD goldens untouched (`git status` clean under
  `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `16.14 40.60 37.77`, after
  `12.48 36.58 36.43`): 21 passed, 27 PASS lines, 0 MISS (no new
  checkpoint; the 13B-2c-2b conversion line: 0.128 ms cancel-to-return at
  checkpoint 53, bound 1.019 ms, PASS).
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load before
  `12.04 36.09 36.25`, after `26.15 25.47 31.25`).
- `cargo run --release --example phase13b_tile_sharing`, `placed_source`
  (24 MP): AFTER (load `49.67 51.89 31.32` → `47.78 51.46 31.29`) —
  `place`: `layer_pixels` 96000000 (384 allocations), `embedded` 96000000,
  `tile_structure` 6144, 0 views, `unique_retained_bytes` 192018589; `dab`:
  `layer_pixels.unique_bytes` **97048576** (388 allocations,
  referenced 289048576), `embedded` 96000000, `tile_structure` 9216,
  **0 views**, `unique_retained_bytes` 193071429. BEFORE (core/src at
  95e09b4, the example and tests unchanged; load `42.02 50.11 31.05` →
  `56.97 51.65 32.63`) — `place`: the same figures but `tile_structure`
  3072 (the asset was contiguous); `dab`: `layer_pixels` **192000000** (768
  allocations), **96000000 view bytes** (1 view), `unique_retained_bytes`
  384019805. `placed_source/save` `rz_sha256`
  `e931ea43a05a7beb6ca4f8f927bfb0166a90bcccb54434e20368ebc03373d5e7`
  (148289175 bytes) in BOTH.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-2c/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `7.52 15.01 24.03`) — ALL
  GATES PASS, P11 parked count 0 through 200 edits;
  `verify_phase12h_conversion.py` (`12h`, load `7.09 13.73 23.04`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, load `6.77 13.44 22.83`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-2c-2c/9h.json`
  (load before `7.95 12.01 18.87`, after `13.02 15.01 18.46`) — 56 groups +
  pass 2, `passed: true`, `current-layer-commands-ui` included.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.
- **Residual**: `rg -c 'contiguous_image\(\)' core/src` sums to 25 call
  lines: 2c-2b's 23 − `source_asset.rs` + the `retained_source_render.rs`
  Sample-arm bind + the `live_filter_evaluate.rs` filtered-read bind + the
  `native_sources_assets.rs` asset writer.

## Verification record (13B-2c-3a)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_persistence` — 10 passed (new,
  serial; every test builds its fixture and twin, `evict()`s, asserts 0
  views, runs the call, then checks 0 views process-wide and `views_of` 0 on
  every held document): (a) `rz_doc_save_native` of the 6000×4000 corpus,
  of a 300×200 source placed over a 640×480 host and of the warp fixture
  under a 3×3 bend — each stream equals the binding-aware contiguous twin's
  (`RSRC` / `WARP` present), 0 views; (b) `rz_doc_save_psd` (layers) of a
  320×240 document with a raster at (−30, 40), a masked raster and a group
  == twin, 0 views; (c) `rz_doc_flat_save_losses` at JPEG / GIF / PNG on
  partial-alpha, hidden-colour and opaque layers == twin, 0 views (PNG
  still reports the hidden colours); (d) `rz_doc_layer_thumbnail` on the
  corpus layer and a 700×500 layer at (−50, 30) == twin, 0 views; (e)
  `rz_doc_layer_canvas_image` for the canvas-sized, offset, off-canvas
  (transparent canvas), 1×1 and group layers == twin, 0 views; (f)
  `rz_doc_promote_structure` of the bend plus a second layer — every raster
  sample `257·byte` and the cleared WARP original the fixture's bytes
  ×257, 0 views; (g) `rz_doc_content_aware_fill` (64×64 at (100, 100), ring
  16, seed 1, sample 0) on the corpus == twin, 0 views; (h) the reopened
  placed case shares `tiles(300, 200)` = 2 tiles with its asset,
  `layer_pixels` 1228800, `embedded_source_pixels` 240000, 0 views after the
  open, resave byte-identical with 0 views, then a `source_edit` transform
  answers a document; (i) after a 100×100 dab the reopened layer equals the
  dabbed source, carries no retained source (the writer dropped the broken
  binding) and so shares 0 tiles, 0 views; (j) a plain 2-layer reopen and a
  bend reopen (WARP original present) leave 0 views.
- **Against the HEAD `7add307` library** (`git archive 7add307 core` into
  the scratchpad, the new suite copied in): 10 of 10 tests fail with **28
  failing checks** — the 26 post-call view checks of (a)–(j) other than
  (e)'s group case (as designed: `group_projection` returns before the
  bind), (h)'s `shared_tile_count == 2` and (h)'s `layer_pixels ==
  1228800` (a check beyond the spec's list; at HEAD the reopened layer is
  its own decoded plane). Every
  byte / losses / sample equality, (e)'s group case and (i)'s sharing and
  bytes pass at HEAD.
- The regression set (`layer_raster_assets`, `layer_raster_compare`,
  `layer_raster_writers`, `layer_raster_reads`, `layer_raster_tiles`,
  `layer_raster`, `tile_store`, `resources`, `psd_export`,
  `persisted_warp_native`, `persisted_warp_numeric`,
  `retained_source_native`, `linked_source_native`, `raw_source_native`,
  `live_filter_native`, `content_aware_tests`, `content_aware_workflow`,
  `doc_tests`, `preservation_tests`, `precision_conversion`) — all green
  after `layer_raster_writers`' pinned fill view went to 0 (the two fill
  cases had failed with "views left behind: left 0, right 96000000", i.e.
  the new behaviour).
- `make test` — run 1 (load before `38.29 22.64 15.96`, after `44.81 35.75
  22.72`) FAILED on one unrelated test,
  `projective_source_native::duplicate_and_unsorted_projective_targets_refuse_at_both_depths`:
  its refusal message quoted the random tempdir `.tmpngoBCM`, which
  lower-cases to contain `png`, tripping the test's
  `!error.to_lowercase().contains("png")` guard (the refusal itself was the
  table's, "invalid or duplicate projective source target"). Runs 2 and 3
  both green: **2790 passed, 0 failed, 24 ignored** (2780 + 10 new); run 2
  load `42.94 35.64 22.84` → `31.28 34.42 24.74`, run 3 `33.66 34.86 24.95`
  → `14.85 27.91 23.92`. PSD goldens untouched (`git status` clean under
  `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load before `14.14 27.55 23.82`, after
  `8.85 24.54 22.89`): 21 passed, 27 verdict lines — 26 PASS, **1 MISS**:
  13A-5a's 8-bit mask feather blur (factor 8, feather 40), 50.125 ms
  cancel-to-return against a bound of 49.217 ms (the timeline's own 48.2 ms
  gap at the issued count; a path this run does not touch). The same binary
  re-run alone (load `6.11 21.88 21.99`) PASSES that line (bound 47.705 ms).
  Bound not widened; recorded as a load residual.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED.
- `cargo run --release --example phase13b_tile_sharing` (24 MP): BEFORE =
  the HEAD `7add307` library with the new example (load `9.40 14.19 18.54`
  → `9.61 14.15 18.50`); AFTER (load `15.04 15.03 18.71` → `14.39 14.90
  18.64`).
  - `placed_source/save`: `rz_sha256`
    `e931ea43a05a7beb6ca4f8f927bfb0166a90bcccb54434e20368ebc03373d5e7`
    (148289175 bytes) in BOTH; views after the save **0 / 0** AFTER, was
    192000000 bytes / 2 views (548 ms BEFORE, 596 ms AFTER).
  - `placed_source/reopen`: AFTER `layer_pixels.unique_bytes` **96000000**
    (384 allocations, referenced 96000000), `embedded_source_pixels`
    96000000, `tile_structure` 6144, **0 views**, `unique_retained_bytes`
    192026036, 784 ms; BEFORE `layer_pixels` 192000000 (768 allocations),
    `embedded` 96000000, `tile_structure` 9216, 192000000 view bytes (2
    views), `unique_retained_bytes` 480029108, 778 ms.
  - `rect_writers/content_aware_fill`: AFTER **0 / 0**, BEFORE 96000000 /
    1; the other four rect writers 0 / 0 in both. `place` / `dab` lines
    identical in both.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-3a/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `14.61 14.91 18.58`) — ALL
  GATES PASS (13 PASS); `verify_phase12h_conversion.py` (`12h`, load `28.99
  18.15 19.55`) — 16 PASS; `verify_phase12g_clipboard.py` (`12g`, load
  `42.25 21.27 20.64`) — 13 PASS; `verify_phase9h_ui.py --require-current
  --report 13b/13b-2c-3a/9h.json` (load before `10.73 14.86 17.92`, after
  `8.94 11.14 15.45`; screen unlocked — `CGSSessionScreenIsLocked` absent
  from `ioreg`) — 56 groups + pass 2, `passed: true`,
  `current-layer-commands-ui` included.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.
- **Residual**: `rg -n 'contiguous_image\(\)' core/src` — 14 call lines, no
  comment hits (listed in the contract), every one on 13B-2c-3b's row;
  `rg -n 'LayerRaster::new\(' core/src/rzdc_read.rs core/src/warp_native.rs`
  — empty.

## Verification record (13B-2c-3b)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test layer_raster_transforms -- --test-threads=1`
  — 12 passed (new, serial; every test builds its fixture and its
  binding-aware contiguous twin, `cold()`, runs the FFI call on both, checks
  the views, then compares every layer's bytes (`same_bytes`), offset and
  dimensions with the twin's answer): (a) `rz_doc_transform_layer` on a
  300×200 layer at (−30, 40) over a 640×480 base — rotate 30° bilinear,
  rotate 90° (the exact arm), pure translate (identity `ptr_eq`) — 0 views;
  (b) `rz_doc_perspective_layer` mild quad — 0 views; (c)
  `rz_doc_geometry_edit` `perspective_set` on the plain offset layer and on
  a 300×200 placed source (the `render8` Sample arm) — 0 views; (d)
  `rz_doc_rotate90` of the 6000×4000 corpus and `rz_doc_flip_horizontal`
  of the offset case — 0 views; (e) `rz_doc_resize` to 320×240, Lanczos3
  and Nearest — 0 views; (f) `warp_edit` with the `bend()` field on the
  warp fixture — 0 views; (g) `rz_doc_expand_retouch_destination` on the
  offset layer — 0 views, bytes equal a hand-built placement of the source;
  (h) `rz_doc_convert_live_filtered_profile` sRGB → Display P3 on a host
  plus a σ 2 filtered placed source — no input view, process-wide exactly
  `2 · 640·480·4` = 2457600 (the two `rebuilt` seeds); (i)
  `rz_doc_scoped_analysis` histogram of the offset layer — JSON == twin, 0
  views; (j) `rz_doc_live_filter_edit` σ 2 on a placed source — 0 views;
  (k) `rz_doc_content_aware_fill_edit` `new_layer` (64×64 at (200, 150),
  ring 16, seed 1) — no input view, exactly `2 · 640·480·4`; (l)
  `rz_doc_heal_to_new_layer` (plate from `rz_doc_retouch_source`) — 0 views.
- **Against the HEAD `9dc0d3d` library** (`git archive 9dc0d3d core` into
  the scratchpad, the new suite copied in): 10 of 12 tests fail with **14
  failing checks**, exactly the stated split — (a) rotate 30° (1173120
  bytes, `views_of` [1, 0, 1, 1]) and rotate 90° (720000), (b) (752400),
  (c) plain (734496) and placed (240000, the asset), (d) rotate90 corpus
  (288000000) and flip (4406400), (e) Lanczos3 and Nearest (2203200 each),
  (f) (3044), (g) (2812800), (h) (3686400 against 2457600, input
  `views_of` [1, 0]), (i) (240000) and (j) (240000). (a) translate, (k) and
  (l) PASS by construction. Every byte / JSON equality passes at HEAD.
- The regression list (`projective_source_native`,
  `layer_raster_persistence`, `layer_raster_writers`, `transform_tests`,
  `perspective_tests`, `geometry_sets`, `canvas_rotation`, `doc_tests`,
  `persisted_warp_native`, `persisted_warp_numeric`, `retouch_ffi`,
  `doc_retouch`, `content_aware_tests`, `content_aware_workflow`,
  `live_filter_native`, `scoped_statistics`, `psd_export`,
  `layer_raster_reads`, `layer_raster_tiles`, `layer_raster`, `resources`)
  — all green.
- `make test` — run 1 (load `16.62 11.57 10.81` → `33.61 20.58 14.54`)
  stopped on one unrelated load flake,
  `develop_inspection::histogram_and_full_cell_marker_scans_cancel_and_release_reservations`
  ("inspection finished before live cancellation", a typed histogram
  worker race this run does not touch). Run 2 (load `32.87 21.03 14.80` →
  `27.28 21.72 15.52`) failed
  `layer_raster_assets::a_resampled_render_leaves_only_the_asset_view_and_restores_its_tiles`,
  whose pinned "the Sample arm's whole-plane read of the asset" view
  (240000) is now 0 — the new behaviour; the assertion was updated to 0 and
  the test renamed `a_resampled_render_leaves_no_view_and_restores_its_tiles`.
  A `--no-fail-fast` run then passed 2802 / 0 failed / 24 ignored. Runs 3
  and 4 both green: **2802 passed, 0 failed, 24 ignored** (2790 + 12 new);
  run 3 load `4.64 13.26 13.34` → `5.03 9.87 11.93`, run 4 `5.03 9.87
  11.93` → `7.11 9.45 11.46`. PSD goldens untouched (`git status` clean
  under `core/tests/fixtures`).
- `make cancel-bench` (serial, once; load `6.70 9.33 11.41` → `5.44 8.67
  11.07`): 21 passed, 27 verdict lines, **27 PASS, 0 MISS**.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED.
- `cargo run --release --example phase13b_tile_sharing`: BEFORE = the HEAD
  `9dc0d3d` library with the new example (load `5.20 9.22 10.06` → `4.55
  8.95 9.95`); AFTER (load `4.55 8.95 9.95` → `4.35 8.84 9.90`).
  - Cold rows (24 MP, `contiguous_view_bytes` / views, BEFORE → AFTER):
    `transform_layer` 227604512 / 2 → **0 / 0** (783.0 → 794.1 ms),
    `rotate90` 192000000 / 2 → **0 / 0** (96.1 → 98.6 ms), `resize`
    120000000 / 2 → **0 / 0** (334.7 → 333.1 ms), `perspective_layer`
    192000000 / 2 → **0 / 0** (610.7 → 642.0 ms); `layer_pixels.unique_bytes`
    identical in both.
  - Warm rows (3000×2000, see decisions): `warp_edit#1` 48032008 / 2 → 0 /
    0 (699.9 → 685.8 ms), **`warp_edit#2`** 72064016 / 3 → 0 / 0 (**699.6 →
    686.2 ms**), `live_filter_edit#1` 24000000 / 1 → 0 / 0 (207.3 → 207.4
    ms), **`live_filter_edit#2`** 24000000 / 1 → 0 / 0 (**272.3 → 277.1
    ms**). The `#2` deltas (−13.4 ms, +4.8 ms) are the warm-path figure.
  - Every other line (`from_contiguous` … `placed_source/reopen`) is
    identical in both apart from its `ms` figures (`placed_source/save`'s
    `rz_sha256` included).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-2c-3b/…`):
  `verify_phase12e2b_placement.py` (`12e2b`, load `29.86 15.23 12.89` →
  `28.63 16.90 13.61`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `26.40 16.80 13.62`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, → `4.53 10.04 11.44`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report
  13b/13b-2c-3b/9h.json` (load `4.41 9.92 11.39` → `4.18 7.15 9.80`;
  screen unlocked — `CGSSessionScreenIsLocked` absent from `ioreg`) — 56
  groups + pass 2, `passed: true`, `current-layer-commands-ui` included.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.
- **Residual**: `rg -n 'contiguous_image\(' core/src` — the definition line
  `layer_raster.rs:290` only. **13B-2 is closed.**

## Verification record (13B-3a)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cd core && cargo test --release --test precision_image_accessor` — 7
  passed (new): dimensions / storage; `contiguous_rgba16` twice
  `Arc::ptr_eq`, `==` the source `Vec`, its `as_ptr()` equal to the `Vec`'s
  captured pointer (the adoption fact), `contiguous_rgba32` `None` on u16
  and the converse; `to_rgba16` / `to_rgba32` equal sample for sample (f32
  by bits) with a different `as_ptr`, each `Err` sentence on the other
  storage; `ptr_eq` (clone true, equal samples in a second allocation
  false, u16 vs f32 false both ways, a same-storage `convert` true);
  `same_samples` (two allocations equal true both ways, one sample differs
  false, transposed dimensions false, storage mismatch false both ways,
  `-0.0` vs `0.0` false, equal f32 true); `from_rgba32` refuses NaN / ∞ /
  −0.25 / 1.5 with its sentence, `from_rgba32_extended` admits 2.0 / −1.0
  and refuses NaN with its sentence, and a NaN buffer under a control whose
  budget is `1 MiB − 1` answers the RESERVATION refusal at both float
  constructors (the order oracle); `PrecisionDocument::from_image` →
  `rename_layer` (through `rz_precision_doc_edit`) keeps the layer `ptr_eq`
  the source, a full-coverage erase does not (and changes samples);
  `retained_bytes()` equals the `from_rgba16` figure before, during and
  after a bound view (both storages).
- `make test` twice — both green: **2809 passed, 0 failed, 24 ignored**
  (2802 + 7 new). Run 1 load `11.00 29.18 25.34` → `24.94 36.96 30.87`
  (4:44 wall); run 2 `23.26 36.41 30.71` → `7.39 24.06 26.55` (2:27 wall).
  No known load flake fired. `tests/resources.rs` unedited except the
  rename and green; `git status --porcelain core/tests/fixtures` empty.
- `make cancel-bench` (serial, once; load `7.03 23.71 26.41` → `5.14 21.40
  25.46`): 20 passed, 1 failed — **24 PASS, 0 MISS** printed; the failure is
  `regional_group_materialization_cancel::a_concurrently_cancelled_materialization_returns_within_the_model`
  ("the poller stalled past the stage: at return progress 1, stage 6, 36549
  checkpoints"), the KNOWN load residual, which aborted that test after its
  first verdict line (PASS) so its remaining verdict lines were not
  printed. Re-run alone it failed the same way (load `4.45 20.45 25.05`);
  the same test on the HEAD `e7639f0` library (`git archive HEAD core` into
  the scratchpad, load `5.61 19.92 24.78` → `10.59 20.03 24.68`) fails
  identically ("entry 2930 + k 0"), so it is not this run's — it touches no
  typed image. Recorded, not chased.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load `4.81
  13.65 21.20` → `27.00 18.75 22.65`).
- `rg -n 'as_rgba16\(|as_rgba32\(' core/src core/tests core/examples` —
  empty. `rg -n 'make_mut\(&mut .*pixels' core/src` — empty.
- **Residual** `rg -c 'contiguous_rgba(16|32)\(' core/src`: 78 lines in 49
  files = 2 definitions + **76 call lines** (per file in the contract).
- **`as_ptr` review** (`rg -n 'as_ptr\(\)'` over the five files, read by
  hand): `ffi_precision.rs` 2 — `:321` the rgba16 export on its bound view
  and `:345` the unrelated ICC-bytes export; `ffi_float.rs` 1 — `:203` the
  rgba32 export on its bound view; `precision_geometry.rs` 2 — `:203` /
  `:224`, the short-circuits on the two bound views; `retained_source.rs` 0;
  `precision_warp.rs` 0. Outside the five, `precision_color.rs:112/340` key
  a dedup map on a bound view's address (decisions).
- **Raw-address test oracles**: `rg -U -c
  'contiguous_rgba(16|32)\(\)\s*\.(unwrap\(\)|expect\("[^"]*"\))\s*\.as_ptr\(\)'
  core/tests` → exactly the six files: `retained_sources.rs` 3,
  `precision_prototype.rs` 3, `precision_canvas_rotation.rs` 2,
  `float_exact_geometry.rs` 1, `placement_insert.rs` 1,
  `precision_straighten.rs` 1 (the new suite binds its views first).
- **Bind-once review** (every hit read by hand): each call binds ONE view at
  the top of the function or block that owns the read (or in the single
  statement that ends the borrow: `precision_paint.rs:158`'s `.len()`,
  `ffi_evaluation.rs:238`), or once per layer / per binding in a per-layer
  loop — `precision_color.rs:110` (the preparation walk) and `:338` (the
  conversion walk), `precision_color_policy.rs:259`, `psd_export16.rs:363`
  (per layer record), `raw_local_develop.rs:163` (per local variant),
  `warp_native.rs:217` (per WARP binding), `precision_lock.rs:289–314` and
  `precision_adjustment.rs:306–307` (one per image, before the loop).
  Closures that index a view capture a view bound outside them
  (`precision_clipboard.rs:231/233`, `precision_sampling.rs:183`'s
  `|i| pixels[i]`, `precision_warp.rs:85`'s `warp_into` read,
  `precision_paint_tools.rs:249`'s `sample`). Views that used to be
  borrowed inline in a `for` head or an argument are now bound by a `let`
  first (`precision_adjustment.rs:306–307`, `precision_scalar_edit.rs:472`,
  `precision_retouch.rs:388` and its transform source, `ffi_retouch.rs:43`,
  `live_filter_evaluate.rs:409`). The one
  per-sample reader, `precision_style_render::alpha16`, reads `pixel(index)`
  and binds nothing. No call sits inside a per-row or per-sample loop body
  or closure.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3a/…`, the app
  BINARY path, HostApp's private dirs and dummy key):
  `verify_phase12e2b_placement.py` (`12e2b`, load `8.28 9.07 16.13` →
  `8.21 8.94 15.75`) — ALL GATES PASS (13 PASS, P11 parked count 0);
  `verify_phase12h_conversion.py` (`12h`, → `7.41 8.73 15.56`) — 16 PASS;
  `verify_phase12g_clipboard.py` (`12g`, → `5.72 7.08 13.11`) — 13 PASS;
  `verify_phase9h_ui.py --require-current --report 13b/13b-3a/9h.json`
  (load `5.66 7.05 13.06` → `7.28 6.57 11.36`; screen unlocked —
  `CGSSessionScreenIsLocked` absent from `ioreg`) — 56 groups + pass 2,
  `passed: true`.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.

## Verification record (13B-3b-1)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_image_tiles` — 9 passed, 1 ignored
  (new): exact round trips at 1×1, 255×257, 256×256, 257×255 and 1000×700
  on both storages (`to_rgba16/32`, the view, `same_samples` against a
  second construction, which is not `ptr_eq`); one changed sample / `-0.0`
  / a storage mismatch answer `same_samples` false; `pixel(i)` for every `i`
  of a 300×300 through the cross-storage `convert` (u16 → f32 `v/65535`
  bitwise, f32 → u16 `round(v·65535)`); identity (clone, a same-storage
  `convert` and Assign Profile's `with_profile` true, a second construction
  and u16 vs f32 false); the seed (the first view's `as_ptr` is the adopted
  Vec's, `contiguous_view_bytes()` grew by `w·h·bpp` at both storages; after
  `rz_resources_evict_caches(NULL, 0)` a different allocation with equal
  samples, two binds `Arc::ptr_eq`); slack (1×1 from a 1024-capacity Vec:
  `storage_bytes() == 8`, the cache grew by 8192); the report over
  `rz_mixed_resources_report` (600×520, both storages: `precision_pixels` =
  `w·h·bpp` in 9 tiles, `tile_structure` present, `precision_buffer` /
  `precision_views` absent, `contiguous_views` 1 — also over the image and
  its clone — and 0 after an evict, `retained_bytes()` unchanged by a
  bind); the pin (`rz_precision_image_pixels_rgba16` the same pointer twice
  and after an evict, bytes equal the samples; `precision_views` = `w·h·8`,
  `contiguous_view_bytes` 0 and `cache_bytes` 0 after the pin; a clone's pin
  after an evict is a different address and both pins are retained; f32 →
  NULL from the u16 export and the converse); a NaN buffer refused by
  `from_rgba32` leaves `contiguous_view_bytes()` unchanged.
- `cargo test --release --test precision_image_tiles -- --ignored` once:
  `fallback_above_the_cache_bound_stays_contiguous` (8193×8193 f32,
  1,074,003,984 B) passed in 0.47 s (0.90 s wall; load `56.03 33.17
  34.07`): both binds are the constructor's Vec, no view, `precision_buffer`
  and no `tile_structure`, `storage_bytes()` = capacity·16, `same_samples`
  against a 1×1 f32 false both ways.
- `precision_image_accessor` (7), `precision_prototype`,
  `retained_source_work` (4, one new:
  `identity_placement_scratch_charges_the_tiling_transient_at_both_depths`
  — 256×256 vs 512×256 identity placements differ by exactly 65536·12 at 8
  bits and 65536·24 at 16), `layer_raster_tiles`, `resources` — green.
- `make test` twice — both green: **2819 passed, 0 failed, 25 ignored**
  (2809 + 9 + 1 new; +1 ignored). Run 1 load `12.52 22.69 29.37` → `35.85
  29.29 30.34` (4:45 wall); run 2 `33.14 28.83 30.18` → `10.66 20.97 26.75`
  (2:34 wall). `git status --porcelain core/tests/fixtures` empty.
- `make cancel-bench` (serial, once; load `10.77 20.66 26.57` → `13.61
  20.25 26.16`): 21 passed, 0 failed, 0 MISS; the known
  `regional_group_materialization_cancel` residual did NOT fire this run.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load `12.76
  19.85 25.94` → `9.92 15.93 21.76`).
- `phase13b_tile_sharing` `typed/*` (24 MP, one sample each; this tree load
  `23.37 26.61 31.22` → `20.92 25.92 30.89`, the HEAD `09c7875` library
  built from `git archive HEAD core` in the scratchpad with this run's
  example copied in, load `19.33 25.51 30.72` → `16.40 24.55 30.28`):

  | line | figure | HEAD `09c7875` | 13B-3b-1 |
  | --- | --- | --- | --- |
  | `typed/dab` | `precision_pixels` unique / referenced | 1728000000 / 1728000000 | 1728000000 / 1728000000 |
  | | `tile_structure.unique_bytes` | 0 | 27648 |
  | | `contiguous_view_bytes` / views | 0 / 0 | 960000000 / 5 |
  | | `unique_retained_bytes` | 1728014291 | 2688043451 |
  | | `ms` (8 edits) | 831.5 | 1073.7 |
  | `typed/projection` | `ms_cold` / `ms_warm` | 388.8 / 323.5 | 504.8 / 460.3 |
  | | views after (doc + 2 projections) | 0 / 0 | 384000000 / 2 |
  | | `sha256` | `a47b902f…66f3c9` | `a47b902f…66f3c9` (equal) |
  | `typed/save` | `ms` | 447.4 | 568.2 |
  | | views after | 0 / 0 | 192000000 / 1 |
  | | `sha256` / `rz_bytes` | `43c5a617…530fa1` / 192021403 | `43c5a617…530fa1` / 192021403 (equal) |

  Each dab state is still a whole new plane (the typed writers rebuild
  through the constructors until 13B-3d), so its seed lands in the cache:
  the views after the 8 dabs (960 MB, 5 views) and after the projection
  (384 MB: the two projections' seeds) are within `CONTIGUOUS_CACHE_BYTES`.
  The typed ops are slower by one tiling copy per construction plus a view
  materialization on a miss (dab +29 %, projection cold +30 % / warm +42 %,
  save +27 %, one sample each under load); not chased (13B-3b-2 /
  13B-3c/3d/3e remove the copies, 13A-latency owns the timing).
- `rg -n 'as_ptr' core/src/precision_color.rs` — empty. `rg -U -c
  'contiguous_rgba(16|32)\(\)\s*\.(unwrap\(\)|expect\("[^"]*"\))\s*\.as_ptr\(\)'
  core/tests` — empty.
- **Residual** `rg -c 'contiguous_rgba(16|32)\(' core/src`: 78 → **74 lines
  = 2 definitions + 72 call lines** (`ffi_float` 1 → 0, `ffi_precision` 1 →
  0, `precision_color` 2 → 0; `precision_image` 4 → 4: the two constructor
  views gone, the two `pinned_*` bodies added). The HEAD figure was taken on
  the scratch `git archive` copy.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3b-1/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase12e2b_placement.py` (`12e2b`,
  load `7.45 14.65 21.06` → `8.29 13.84 20.42`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `7.64 13.42 20.16`) — 16 PASS;
  `verify_phase12g_clipboard.py` (`12g`, → `5.87 9.95 16.18`) — 13 PASS;
  `verify_phase9h_ui.py --require-current --report 13b/13b-3b-1/9h.json`
  (load `5.56 9.81 16.10` → `8.97 8.72 13.87`; screen unlocked —
  `CGSSessionScreenIsLocked` absent from `ioreg`) — 56 groups + pass 2,
  `passed: true`.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.

## Verification record (13B-3b-2)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_image_writers --test
  precision_image_tiles --test precision_image_accessor --test
  layer_raster_persistence --test precision_straighten --test
  retained_sources --test precision_color --test placement_insert --test
  precision_retouch` — all green: 9 (new) / 9 + 1 ignored / 7 / 10 / 12 /
  12 / 9 / 7 / 7. The new `precision_image_writers` suite: (a) a 1024×768
  u16 rebuild with one 300×300 change across both seams shares 12 − 4
  tiles, is not `ptr_eq`, its samples are exact, the input is unchanged,
  `contiguous_view_bytes` grew by `w·h·8` and the view `as_ptr` is the
  adopted Vec's; (b) a no-change rebuild shares all 12 tiles under a fresh
  identity with ONE view (the base's entry replaced by the new buffer —
  both images' view `as_ptr` is the new Vec's); (c) other dimensions and an
  f32 `self` tile fresh (0 shared, samples exact, seeded), and the length
  refusal is the constructor's sentence; (d) `-0.0` over `+0.0` in one f32
  sample allocates exactly one tile, NaN and 70000 are refused with
  `from_rgba32_extended`'s sentence; (e) `with_rect_rgba16` over an
  interior rect, each edge, the last sample, a 1×1 rect, the seam-crossing
  300×300 (4 fresh tiles) and the whole plane (12) against a hand overwrite,
  the profile shared, 0 view bytes after; a 1×1 plane; zero area and an
  unchanged rect `Ok(None)`; a rect leaving the plane, a count mismatch and
  the wrong storage `Err`; the f32 twin (samples, `-0.0` one tile, NaN /
  −70000 refused, leaving `Err`), no view left; (f) `read_rect_rgba16/32`
  against hand slices at five rects, `None` for zero area, leaving the plane
  and the other storage, no view bound; (g) `from_rgba16_unseeded`: 0 view
  bytes, `tile_structure` in the report, `precision_pixels` `w·h·8`, then
  `contiguous_rgba16()` materializes exactly `w·h·8`; (h) a typed identity
  Place (600×300): the layer is not `ptr_eq` to
  `rz_precision_doc_source_master16`'s image, which is `ptr_eq` to the
  input's layer; `same_samples`; all 6 tiles shared; the placed document's
  `precision_pixels.unique_bytes` = `w·h·8`; (i) two `rz_precision_doc_edit`
  paint dabs on a 1024×768 document retain `precision_pixels.unique_bytes ≤
  w·h·8 + 2·4·(256·256·8)` over the three states.
- `make test` twice — both green: **2828 passed, 0 failed, 25 ignored**
  (2819 + 9 new). Run 1 load `20.66 27.30 22.46` → `40.60 44.89 34.75`
  (≈6 min wall); run 2 → `8.35 29.40 30.06` (≈3 min wall). `git status
  --porcelain core/tests/fixtures` empty.
- `make cancel-bench` (serial, once; load `5.52 10.03 18.67` → `5.49 9.44
  18.06`): 21 passed, 0 failed, 0 MISS; the known
  `regional_group_materialization_cancel` residual did NOT fire this run.
- `rg -n 'mod (plane|build);' core/src/precision_image.rs` — both present,
  each under its `#[path]` (lines 66–69); `git diff --stat core/src/lib.rs`
  empty. `wc -l`: `precision_image.rs` 439, `precision_image_plane.rs` 238,
  `precision_image_build.rs` 485.
- `make typecheck` — 0 errors. `make app` — BUILD SUCCEEDED (load `5.47
  9.24 17.84` → `9.91 9.49 16.21`).
- `phase13b_tile_sharing` `typed/*` with `RZ_TILE_SHARING_ONLY=typed` (24 MP),
  3 samples each: HEAD `634746c` built from `git archive HEAD core` in the
  scratchpad with this run's example copied in, sampled TWICE — set A first
  (load `23.02 18.74 13.40` / `21.34 18.46 13.34` / `20.35 18.31 13.31`), set
  B after this run's tests (load `5.76 10.73 19.32` / `5.46 10.58 19.22` /
  `5.34 10.47 19.13`) — and this tree (load `6.46 11.58 20.03` / `6.34
  11.47 19.94` / `5.99 11.31 19.83`):

  | line | figure | HEAD A (3 samples) | HEAD B (3 samples) | 13B-3b-2 (3 samples) |
  | --- | --- | --- | --- | --- |
  | `typed/dab` | `precision_pixels` unique / referenced | 1728000000 / 1728000000 | 1728000000 / 1728000000 | **208777216** / 1728000000 |
  | | `tile_structure.unique_bytes` | 27648 | 27648 | 27648 |
  | | `contiguous_view_bytes` / views | 960000000 / 5 | 960000000 / 5 | 960000000 / 5 |
  | | `unique_retained_bytes` | 2688043451 | 2688043451 | **1168820667** |
  | | `ms` (8 edits) | 949.6 / 873.0 / 904.1 | 856.6 / 848.7 / 848.5 | 864.7 / 839.7 / 836.4 |
  | `typed/projection` | `ms_cold` | 482.1 / 445.2 / 441.1 | 432.2 / 434.1 / 441.2 | 436.6 / 441.0 / 439.3 |
  | | `ms_warm` | 423.5 / 419.7 / 411.0 | 406.5 / 403.7 / 409.0 | 411.3 / 412.1 / 412.2 |
  | | views after (doc + 2 projections) | 384000000 / 2 | 384000000 / 2 | 384000000 / 2 |
  | | `sha256` | `a47b902f…66f3c9` | `a47b902f…66f3c9` | `a47b902f…66f3c9` (equal) |
  | `typed/save` | `ms` | 423.9 / 372.3 / 252.3 | 279.9 / 242.7 / 251.1 | 406.1 / 242.3 / 401.5 |
  | | views after | 192000000 / 1 | 192000000 / 1 | 192000000 / 1 |
  | | `sha256` / `rz_bytes` | `967ed9b6…9d2db2` / 192021403 | `967ed9b6…9d2db2` / 192021403 | `967ed9b6…9d2db2` / 192021403 (equal) |
  | `typed/promote` | `ms` | 68.3 / 67.0 / 65.2 | 65.0 / 64.7 / 59.7 | 60.7 / 66.1 / 59.7 |
  | | `contiguous_view_bytes` / views | 192000000 / 1 | 192000000 / 1 | **0 / 0** |
  | | `precision_pixels.unique_bytes` | 192000000 | 192000000 | 192000000 |
  | | `ms_first_projection` | 409.0 / 407.1 / 407.0 | 407.4 / 424.0 / 404.4 | 412.3 / 418.7 / 412.2 |
  | | views after the first projection | 192000000 | 192000000 | **0** |

  The filtered run's `typed/save` hash differs from 13B-3b-1's recorded
  `43c5a617…530fa1` because the save carries the layer ids (`LIDS`) and the
  filter skips the byte ops that mint ids first; it is equal between HEAD
  and this tree. ONE unfiltered run of this tree (load `5.29 10.29 18.97`
  → `5.39 10.08 18.74`) gives `typed/save` `43c5a617…530fa1` / 192021403
  and `typed/projection` `a47b902f…66f3c9` — 13B-3b-1's record exactly —
  with `typed/dab` 208777216 and `typed/promote` 0 views.

  `typed/dab`'s `precision_pixels.unique_bytes` falls to exactly the bound
  `192000000 + 8·4·524288 = 208777216` (8 dabs × 4 tiles), `tile_structure`
  still 9 indices. The views after the dabs are unchanged (each `rebuilt`
  seeds its buffer; 13B-3d's rect writes seed nothing). Wall time is flat
  within noise at the same load (dab median 839.7 vs 848.7 ms on HEAD B):
  each site still builds and scans a whole-plane Vec and `rebuilt` still
  compares every tile against it (copying only the changed ones), so
  13B-3b-2 removes memory, not time; the save's spread (242–406 ms) is noise present at HEAD too. Not
  chased (13A-latency).
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: 74 = 2 definitions +
  72 call lines, unchanged (HEAD 74 on the scratch archive).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3b-2/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase12e2b_placement.py` (`12e2b`,
  load `10.00 7.29 11.74` → `8.06 7.14 11.48`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `7.67 7.09 11.41`) — 16 PASS;
  `verify_phase12g_clipboard.py` (`12g`, → `11.53 9.15 10.85`) — 13 PASS;
  `verify_phase9h_ui.py --require-current --report 13b/13b-3b-2/9h.json`
  (load `11.56 9.20 10.86` → `11.39 11.13 11.23`; screen unlocked —
  `CGSSessionScreenIsLocked` absent from `ioreg`) — 56 groups + pass 2,
  `passed: true`.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` — empty afterwards; no
  `scripts/**/__pycache__`.

## Verification record (13B-3c-1)

All run on 2026-09-25 on this machine (the user's other work running, and
another session's own Rasterize instance on port 4917; load recorded beside
every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_image_reads --test
  precision_compositor_parity` — 6 passed + 1 ignored / 9 passed. The new
  `precision_image_reads` suite: (a) `row_segments` on 600×520 u16 and f32
  images at y ∈ {0, 255, 256, 519} over (0, w), (250, 262), (255, 256),
  (599, 600), (0, 10000), (300, 300), (400, 300), and y = 520: runs adjoin,
  stay inside one tile column, concatenate to the hand slice, carry the
  storage's variant, `pixel(j)` bitwise the hand-normalized sample; (b)
  `sample_at` at every tile corner / seam, `None` at (w, 0), (0, h),
  (u32::MAX, u32::MAX); (c) `runs_rgba16/32` flatten = `to_rgba16/32` at
  600×520, 1×1 and 257×3, `None` for the other storage; (d) after `evict`
  every primitive at both storages leaves `contiguous_view_bytes()` 0; (e)
  after `evict`, a projection and a 300×200 regional window of a three-layer
  600×520 u16 document and of a three-raster float document leave 0
  `contiguous_view_bytes` over the DOCUMENT, and process-wide exactly the two
  outputs' seeds (`w·h·bpp` each, 13B-3c-2's item pinned); (f) a horizontal
  flip of a mirror-symmetric 600×520 layer (ruler origin at the centre)
  answers `None` and one with a single asymmetric sample at (599, 519)
  answers a changed document, at u16 and f32; (g) `#[ignore]`.
- `cargo test --release --test precision_image_reads -- --ignored
  --test-threads=1` (load `37.75 28.92 27.85`) — the 8193² f32 contiguous
  fallback: `sample_at`, `row_segments` (ONE run each) and `runs_rgba32`
  against the per-index formula over all 67,125,249 samples, 0 view bytes;
  passed (0.88 s).
- The listed 16 suites (`precision_image_tiles`, `_writers`, `_accessor`,
  `precision_layer_blending`, `precision_layer_styles`, `precision_knockout`,
  `precision_groups`, `precision_clipping`, `regional_precision` and its
  `_styles` / `_knockout` / `_styled_groups` / `_masks_clips`,
  `precision_geometry`, `float_exact_geometry`, `geometry_noops`) — all
  green (16 `test result: ok`); `precision_layer_blending`'s known
  `blended_render_cancellation_…` flake did not fire.
- `make test` twice — both green: **2843 passed, 0 failed, 26 ignored**
  (2828 + 15 new; 25 + 1 new ignored). Run 1 load `26.24 18.91 14.37` →
  `49.82 53.14 34.62`; run 2 `44.41 51.87 34.38` → `11.22 35.08 30.44`.
  `git status --porcelain core/tests/fixtures` empty.
- `make cancel-bench` (serial, once; load `10.49 34.53 30.27` → `11.88
  32.00 29.54`): 21 passed, 0 failed, 0 MISS; the known
  `regional_group_materialization_cancel` residual did NOT fire.
- `make typecheck` — 0 errors (pre-existing warnings only). `make app` —
  BUILD SUCCEEDED (load `10.28 31.00 29.22` → `28.07 23.46 25.94`).
- `precision_compositor_parity -- --nocapture` on the HEAD `740294b`
  archive (`git archive HEAD core` in the scratchpad, this run's parity file
  copied in) and on this tree: all 14 `parity` lines identical —

  | fixture | u16 `sha256` | f32 `sha256` |
  | --- | --- | --- |
  | `off_canvas` | `d2ed28d7…b11695` | `b254a75e…93d704` |
  | `seams` | `756f78aa…aea157` | `bb4a6439…e96e4d` |
  | `masked` | `7e8eb598…6fde5` | `830d32d9…923c5` |
  | `multiply` | `89157cf6…70a1c` | `7a0c6034…c4351` |
  | `blend_if` | `5907e1de…d08d2` | — (refused at float) |
  | `styled` | `cd6bfeb0…37d24` | — |
  | `knockout` | `6fc46be7…71ef9` | — |
  | `styled_group` | `1aa5f5fa…95e4f` | — |
  | `wide` | `cc433c1f…96da8` | `6dfca63b…f6e6d` |

- `phase13b_tile_sharing` with `RZ_TILE_SHARING_ONLY=typed` (24 MP; float
  3000×2000), HEAD `740294b` built from the scratch archive with this run's
  example copied in, alternated with this tree, 3 samples each, TWICE: set A
  under load (`42.57 27.87 27.43` … `36.95 28.61 27.74`), set B at the
  lowest available load (`5.91 11.03 17.34` … `6.64 10.72 17.01`, every
  sample within `5.84`–`6.69`). Set B:

  | line | figure | HEAD (3 samples) | 13B-3c-1 (3 samples) |
  | --- | --- | --- | --- |
  | `typed/dab` | `ms` (8 edits) | 885.5 / 863.4 / 858.0 | 888.8 / 831.6 / 876.3 |
  | | views after | 960000000 | 960000000 |
  | `typed/projection` | `ms_cold` | 459.2 / 454.9 / 446.7 | **359.1 / 354.9 / 352.6** |
  | | `ms_warm` | 424.8 / 421.9 / 412.4 | **320.9 / 317.1 / 318.2** |
  | | `contiguous_view_bytes_document` | 0 | 0 |
  | | views (doc + 2 projections) | 384000000 | 384000000 |
  | | `sha256` | `a47b902f…66f3c9` | `a47b902f…66f3c9` (equal) |
  | `typed/save` | `ms` | 560.9 / 388.9 / 309.6 | 409.5 / 360.9 / 261.6 |
  | | `sha256` / `rz_bytes` | `967ed9b6…9d2db2` / 192021403 | equal |
  | `typed/regional_window` | `ms_cold` | 28.5 / 26.7 / 27.9 | 28.0 / 27.5 / 37.3 |
  | | `ms_warm` | 26.0 / 26.1 / 26.2 | 25.9 / 26.6 / 26.1 |
  | | `contiguous_view_bytes_document` | 0 | 0 |
  | | views (doc + 2 windows) | 16777216 | 16777216 |
  | | `sha256` | `8ef40271…aa8039` | `8ef40271…aa8039` (equal) |
  | `typed/float_projection` | `ms_cold` | 87.1 / 89.0 / 89.4 | **65.2 / 67.4 / 60.8** |
  | | `ms_warm` | 87.3 / 116.6 / 91.9 | **71.8 / 72.3 / 70.9** |
  | | `contiguous_view_bytes_document` | 0 | 0 |
  | | views (doc + 2 projections) | 192000000 | 192000000 |
  | | `sha256` | `ffc6858f…887adc5` | `ffc6858f…887adc5` (equal) |
  | `typed/promote` | `ms_first_projection` | 442.7 / 403.8 / 404.4 | **306.9 / 311.6 / 310.8** |
  | | views after the first projection | 0 | 0 |

  Set A (load ~37–43) gives the same hashes and view figures with noisy
  timings (projection cold HEAD 443.7 / 534.0 / 499.3, this tree 745.6 /
  434.4 / 370.9). The document-side view figure is 0 at HEAD as well: since
  13B-3b-1 the compositor's `pixel(index)` already read the tiles one sample
  at a time, so this run's gain is TIME (the full projection ~21–25 % faster
  at 24 MP u16, ~25 % at 6 MP float, the first projection after a promotion
  ~23 %: a borrowed run per tile row instead of a tile lookup per sample),
  while the remaining views are the OUTPUTS' own seeds (2 × 192000000 /
  2 × 8388608 / 2 × 96000000), 13B-3c-2's item. The typed regional window
  (the per-sample path, which still looks up one sample per call) is flat
  within noise (warm 26.0–26.2 vs 25.9–26.6 ms) — no regional speed change
  to hand 13A-latency.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **71 = 2
  definitions + 69 call lines** in 45 files (HEAD 74 on the scratch archive;
  `precision_geometry` −3).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3c-1/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase12e2b_placement.py` (`12e2b`,
  load `20.10 25.34 26.58` → `13.80 22.89 25.61`) — ALL GATES PASS (13
  PASS); `verify_phase12h_conversion.py` (`12h`, → `12.42 22.30 25.37`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, → `4.25 12.17 19.95`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 13b/13b-3c-1/9h.json`
  (load `3.82 11.81 19.73` → `7.85 11.72 17.73`; screen unlocked —
  `CGSSessionScreenIsLocked` absent from `ioreg`) — 56 groups + pass 2,
  `passed: true`.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists ONE process,
  not this run's: pid 68091 on port 4917, started 16:57:22 by another
  session's `spike/launch-app.sh` (a different scratchpad); left untouched.
  No `scripts/**/__pycache__`.

## Verification record (13B-3c-2a)

All run on 2026-09-25 on this machine (the user's other work running, and
another session's own Rasterize instance — pid 38071, port 4917, a
`.claude/worktrees/downloadable-deploy` checkout — left untouched; load
recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_projection_consumers --test
  precision_image_reads -- --nocapture --test-threads=1` — 16 passed / 7
  passed + 1 ignored. `precision_projection_consumers` (fixtures 600×520,
  257×3, 1×1; u16 codes with exact 0 / 65535 channels, all-black and
  all-white RGB, alpha 0 and 65535; extended f32 through a float TIFF,
  the raster checked bit-for-bit against the TIFF samples): quantize None /
  Ordered and `to_rgba8` equal the written-out rules (Bayer table copied),
  `quantize_scratch_bytes` = 1 MiB + `w·h·4`; histogram bins / alpha bins /
  `total` / `sampled_pixels` / exact endpoints at strides 1, 7, 4097; sample
  sums / count / centre at reach 0–2 over corners, seams and partly / wholly
  off-image centres (the "outside" sentence), the storage gate before the
  reach check; compare `max_abs` / `mean_abs` / `changed_fraction` for a
  differing and an identical pair, both refusals; the scoped Layer-source
  histogram (strides 1, 7) and a reach-1 sample with the layer partly off
  the canvas; the stop-binned float histogram and the float sample
  (`stops`, above-white / below-zero); `display_rgba8` and the FOGRA39 proof
  (`{"gamut_warning":true}`; FOGRA39 is installed) at 72 / 12 / 1 seam /
  corner positions (600×520 / 257×3 / 1×1) each equal to a 1×1 image's
  answer. `no_view_*` (7 tests):
  after `evict` every consumer leaves `contiguous_view_bytes()` 0 — on the
  HEAD archive all 7 FAIL as designed (4992000 / 2496000 / 480000 bytes
  left). `precision_image_reads` (b2): `rgba16_at` = the source code at
  every tile corner / seam, `None` at (w, 0), (0, h), (u32::MAX,
  u32::MAX) and for f32, 0 views. `-- --ignored` (load `12.28 8.17
  13.13`): the 8193² contiguous fallback (g) passed (0.69 s).
- The 12 listed suites (`precision_stats`, `float_document`,
  `float_interchange`, `evaluation_ffi`, `soft_proof_tests`,
  `scoped_statistics`, `precision_conversion`, `precision_conversion_narrow`,
  `export_production`, `develop_inspection`, `precision_prototype`,
  `precision_ffi`) — all green (6 / 14 / 6 / 3 / 11 / 7 / 10 / 14 / 16 / 7
  / 9 / 4). The first invocation, run while the HEAD archive compiled
  beside it (load ~52–61), failed `develop_inspection::
  histogram_and_full_cell_marker_scans_cancel_and_release_reservations`
  once ("inspection finished before live cancellation": the main thread
  cancels after the worker's stage 6 and the 4 MP histogram finished
  first); the rerun was green, and 30 alternated runs of that test on the
  HEAD archive and on this tree at load ~50 failed 0 / 30 each — a load
  race in the test, not a changed checkpoint (the `histogram` `consumer`
  lines' checkpoint and report hashes are HEAD's).
- `consumer` lines, HEAD `656a0aa` archive (`git archive HEAD core` in the
  scratchpad with this run's suite copied in) vs this tree, `record_ --
  --nocapture --test-threads=1`: all 33 lines (11 labels × 3 fixtures)
  IDENTICAL — answer `sha256`, `checkpoints=` and `reports=` (e.g.
  `preview_rgba8 600x520 cc663006…bcca30 checkpoints=80`, `histogram
  600x520 79548249…29bd8 checkpoints=104`, `display_rgba8 600x520
  d2a4a8a0…b8f5e checkpoints=160`, `proof_render 600x520 2c4cf95f…a4a67
  checkpoints=78`, `scoped_layer_histogram 600x520 3667d7c6…49841
  checkpoints=175`).
- `make test` twice — both green: **2860 passed, 0 failed, 26 ignored**
  (2843 + 17 new). Run 1 load `16.20 25.53 18.66` → `35.74 49.92 33.09`;
  run 2 `32.05 48.66 32.84` → `7.94 31.61 28.40`. `git status --porcelain
  core/tests/fixtures` empty.
- `make cancel-bench` (serial, once; load `5.77 27.04 26.90` → `5.77 24.70
  26.06`): 20 passed, 1 failed — the known load residual
  `regional_group_materialization_cancel::a_concurrently_cancelled_materialization_returns_within_the_model`
  ("the poller stalled past the stage"; byte regional code this run does not
  touch); not chased, per the load policy.
- `make typecheck` — 0 errors (pre-existing warnings only). `make app` —
  BUILD SUCCEEDED (load `5.11 15.96 22.14` → `19.77 18.66 22.94`).
- `phase13b_tile_sharing` with `RZ_TILE_SHARING_ONLY=consumers` (24 MP u16
  corpus; float 3000×2000), the HEAD archive's build (this run's example
  copied in) alternated with this tree, 3 samples each, TWICE: set A (load
  `46.62` → `24.54`, falling) and set B (load `8.02 31.23 28.29` … `5.84
  27.41 27.03`). Every `sha256` equal HEAD vs tree in both sets;
  `contiguous_view_bytes_after_cold` 0 for every consumer on the tree.
  Set B (ms, 3 samples, median ratio tree / HEAD):

  | consumer | depth | figure | HEAD | 13B-3c-2a | ratio |
  | --- | --- | --- | --- | --- | --- |
  | `preview_rgba8` | u16 | `ms_cold` | 98.96 / 92.47 / 91.94 | 57.82 / 57.95 / 57.35 | 0.63 |
  | | | `ms_warm` | 73.62 / 73.48 / 73.87 | **55.12 / 54.17 / 55.43** | **0.75** |
  | | | views after cold | 192000000 | 0 | |
  | `quantize_ordered` | u16 | `ms_cold` / `ms_warm` median | 116.08 / 77.57 | 97.13 / 76.21 | 0.84 / 0.98 |
  | `histogram` | u16 | `ms_cold` / `ms_warm` median | 130.16 / 115.05 | 169.01 / 166.85 | 1.30 / **1.45** |
  | `sample` | u16 | `ms_cold` median | 13.94 | 0.02 | — |
  | `compare` | u16 | `ms_cold` / `ms_warm` median | 115.70 / 87.17 | 325.67 / 326.85 | 2.81 / **3.75** |
  | | | views after cold | 384000000 | 0 | |
  | `proof_render` | u16 | `ms_cold` / `ms_warm` median | 719.76 / 681.55 | 699.70 / 677.49 | 0.97 / 0.99 |
  | `scoped_layer_histogram` | u16 | `ms_cold` / `ms_warm` median | 275.15 / 259.15 | 308.16 / 310.09 | 1.12 / **1.20** |
  | `display_rgba8` | f32 | `ms_cold` | 764.09 / 783.17 / 782.65 | 770.35 / 777.57 / 801.82 | 0.99 |
  | | | `ms_warm` | 738.62 / 746.98 / 745.76 | **785.23 / 775.23 / 764.63** | **1.04** |
  | | | views after cold | 96000000 | 0 | |
  | `float_histogram` | f32 | `ms_cold` / `ms_warm` median | 151.02 / 142.51 | 158.76 / 162.19 | 1.05 / **1.14** |
  | `float_sample` | f32 | `ms_cold` median | 11.30 | 0.01 | — |

  `sha256` (both sets, HEAD = tree): `preview_rgba8` and `quantize_ordered`
  `563f6179…833acf` (a `v·257` code survives every Bayer threshold),
  `histogram` `a7c9f3d8…0cdaa2`, `sample` `48f8cd71…93eb84`, `compare`
  `07edeaec…d86d14`, `proof_render` `c5a37871…5dfff2`,
  `scoped_layer_histogram` `e261cc23…b4a3c8`, `display_rgba8`
  `f8d31b03…a9632e`, `float_histogram` `fdcd9363…5a3980`, `float_sample`
  `e9b07815…afccc8`. The fixed bound (warm `preview_rgba8` and warm
  `display_rgba8` ≤ 1.10 × HEAD median at equal load) is MET in both sets
  (set A: 0.74 / 0.97). Set A's ratios for the unbounded walks agree
  (compare warm 3.66 ×, histogram 1.47 ×, scoped 1.20 ×, float histogram
  1.18 ×). The cold figures drop where HEAD's first call materialized the
  view (preview −37 %, sample and float sample ~14 / 11 ms → ~0); the
  flattened stats walks are slower warm than HEAD's indexed slice over a
  cached view — recorded as an open item (13A-latency).
- **Review fix pass** (compare and both histograms rewritten, see the
  contract): `record_*` HEAD archive vs tree — all 33 `consumer` lines
  identical (answer sha256, checkpoints, report-log hash). `typed/consumers`
  HEAD/tree alternated, 3 samples each, load `16.82 20.64 18.53` → `13.79
  19.49 18.21`; every `sha256` equal, every tree
  `contiguous_view_bytes_after_cold` 0 (ms medians, tree / HEAD):

  | consumer | `ms_cold` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio |
  | --- | --- | --- | --- | --- |
  | `preview_rgba8` | 91.87 → 59.16 | 0.64 | 74.90 → 56.54 | **0.75** |
  | `quantize_ordered` | 115.85 → 96.96 | 0.84 | 78.12 → 77.39 | 0.99 |
  | `histogram` | 132.86 → 122.86 | 0.92 | 116.17 → 124.28 | **1.07** |
  | `compare` | 117.16 → 50.02 | 0.43 | 89.92 → 48.52 | **0.54** |
  | `proof_render` | 730.36 → 714.47 | 0.98 | 685.72 → 684.63 | 1.00 |
  | `scoped_layer_histogram` | 277.75 → 317.04 | 1.14 | 266.10 → 306.47 | 1.15 |
  | `display_rgba8` | 808.56 → 784.98 | 0.97 | 759.23 → 763.11 | **1.01** |
  | `float_histogram` | 153.90 → 141.10 | 0.92 | 141.62 → 141.86 | **1.00** |

  The fixed bound stays MET (preview 0.75 ×, display 1.01 ×).
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **61 = 2
  definitions + 59 call lines** in 38 files (HEAD 71 in 45 files).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3c-2a/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase12e2b_placement.py` (`12e2b`,
  load `21.05 19.25 23.04` → `34.10 23.16 24.32`) — ALL GATES PASS (13
  PASS); `verify_phase12h_conversion.py` (`12h`, → `29.35 22.65 24.12`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, → `5.17 12.81 19.36`) — 13
  PASS; `verify_phase11f_float.py --output 13b-3c-2a/11f` (tone map and float
  display; → `4.82 12.36 19.09`) — ALL GATES PASS (11 PASS);
  `verify_phase9h_ui.py --require-current --report 13b-3c-2a/9h.json` (load
  `4.76 12.22 19.00` → `4.67 9.46 16.00`) — 55 PASS, 1 FAIL:
  `current-layer-commands-ui` ("The owned editor never became the real key
  window") with the screen LOCKED (`CGSSessionScreenIsLocked = true` in
  `ioreg`), the known lock-state failure; rerun alone
  (`RZ_PAINT_UI_CASES=current-layer-commands-ui`,
  `9h-current-layer-commands-ui.json`) — the same failure, screen still
  locked. Run logs copied beside the reports.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists ONE process,
  not this run's (pid 38071, above); left untouched. No
  `scripts/**/__pycache__`.

## Verification record (13B-3c-2b)

All run on 2026-09-25 on this machine (the user's other work running, and
another session's own Rasterize instance — pid 38071, a `./build/…`
launch — left untouched; load recorded beside every timing).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_projection_unseeded --test
  precision_image_reads --test precision_projection_consumers --
  --nocapture --test-threads=1` — 8 passed / 7 passed + 1 ignored / 16
  passed. `precision_projection_unseeded` (fixtures 600×520, 257×3, 1×1;
  the u16 document of `precision_image_reads::u16_document`'s shape and
  the float document of `float_multi_document`'s, scaled to each fixture):
  Copy Merged over the whole canvas, a window across both seams and the
  last pixel, with no coverage and a 0 / 1 / 32768 / 65534 / 65535
  pattern — samples, alpha, `offset()` and `coverage()` equal to the
  render's owned samples sliced here; Copy of "Seam" (130, 140, 390×260) at
  both depths over a window crossing its corner, (0, 200, 100, 10) left of
  it, (530, 200, 60, 10) right of it, (0, 0, 600, 100) above it and one
  inside across the seams — zeros where it does not reach; the narrow
  preview's `Rgba8` arm against `render().to_rgba16()` vs `flattened()`
  ×257 and its `Rgba16` arm (delta 0, counts from the alpha-zero pixels;
  alpha 0 / 1, decision above); the promote preview for a one-layer (delta
  0) and a two-layer (opacity 0.5) byte document; the preserve-appearance
  bake to Display P3 (`action` "baked", the conversion succeeds) and its
  refusal with one alpha 65534 at (256, 256) and at (599, 519), preflight
  and conversion alike; `no_view_copy_layer` (0 views over the document
  after Copy at both depths), `no_view_scene_render` (0 process-wide after
  `PrecisionScene::render` at both storages) and
  `seeded_outputs_keep_their_view` (exactly `W·H·8` after `flatten`,
  `merge_visible`, `stamp_visible`). `precision_image_reads` (e): 0
  process-wide after a projection and a window at both depths.
  `record_float_histogram`: the stride-1 blue minimum is −0.0 bitwise.
- HEAD comparison (`git archive HEAD core` into a scratch directory, the
  new suite, the edited consumers suite and the example copied in): at HEAD
  `no_view_copy_layer` and `no_view_scene_render` FAIL (by design),
  `seeded_outputs_keep_their_view` passes; `record_*` HEAD vs tree — all 23
  `unseeded` lines and all 33 `consumer` lines identical (answer sha256,
  checkpoints, report-log hash).
- The regression set (`precision_clipboard`, `_boundaries`, `_editable`,
  `precision_conversion`, `_narrow`, `_structure`, `conversion_policy_tests`,
  `precision_merge`, `precision_stamp_flatten`, `precision_scalar_selection`,
  `scoped_statistics`, `precision_retouch`, `precision_inpaint`,
  `precision_projection_export`, `export_pipeline`, `float_interchange`,
  `float_document`, `precision_layer_styles`, `precision_masks`,
  `precision_prototype`, `precision_ffi`) — all green.
- `make test` twice: 290 suites, 2868 passed, 0 failed, 26 ignored each
  (run 1 load `4.00 11.02 13.10` → `15.52 20.13 17.36`; run 2 → `10.89
  15.57 15.89`).
- `make cancel-bench` once, serial (load `9.41 12.67 14.59` → `9.76 12.33
  14.37`): 20 passed, 1 FAILED —
  `regional_byte_preparation_cancel::the_byte_guide_blurs_each_report_inside_their_sub_ranges`
  ("the byte guide's first blur: progress 0.566065 logged at 297.388 ms
  lies outside the range [0.5288, 0.5658]"), an 8-bit RAMP guard over a
  path this run does not touch. Rerun alone: tree FAIL then, alternated
  with the HEAD `694194c` library, tree 3 / 3 and HEAD 4 / 4 PASS at load
  ~11–13 — a load flake of the poller, reported, not chased.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **55 = 2
  definitions + 53 call lines** in 36 files (HEAD 61 in 38 files).
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=typed` then `=unseeded`,
  HEAD (the `694194c` library with this example) and tree alternated, 3
  samples each (`typed` load `14.13 19.12 15.78` → `6.46 14.31 14.41`;
  `unseeded` → `4.59 11.61 13.35`); every `sha256` equal HEAD / tree.
  Memory: `typed/projection` `contiguous_view_bytes` **0** (was 384000000),
  `typed/float_projection` **0** (was 192000000), `typed/regional_window`
  **0** (was 16777216); `contiguous_view_bytes_document` 0 on both. Bounded
  ms medians (tree / HEAD, bound ≤ 1.10, all MET):

  | line | `ms_cold` / `ms_first` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio |
  | --- | --- | --- | --- | --- |
  | `typed/projection` | 358.43 → 355.52 | 0.99 | 322.16 → 330.55 | 1.03 |
  | `typed/float_projection` | 66.39 → 68.43 | 1.03 | 67.86 → 69.30 | 1.02 |
  | `typed/regional_window` | 27.32 → 26.82 | 0.98 | 26.91 → 25.46 | 0.95 |
  | `copy_merged` (`typed` run) | 390.47 → 362.06 | 0.93 | 382.16 → 350.93 | 0.92 |
  | `copy_merged` (`unseeded` run) | 445.93 → 419.71 | 0.94 | 385.60 → 358.63 | 0.93 |

  Recorded without a bound (`unseeded` run; `contiguous_view_bytes_after_first`
  equal HEAD / tree — the inputs' own seeds plus, on the tree, the export
  readers' one materialization where HEAD had the seed):

  | consumer | `ms_first` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio |
  | --- | --- | --- | --- | --- |
  | `narrow_preview` (f32 3000×2000) | 1719.18 → 1719.55 | 1.00 | 1698.50 → 1710.52 | 1.01 |
  | `promote_preview` | 578.59 → 595.60 | 1.03 | 577.68 → 576.78 | 1.00 |
  | `export_png16` | 93.31 → 124.49 | 1.33 | 96.39 → 99.91 | 1.04 |
  | `export_png16_half` | 518.39 → 544.97 | 1.05 | 524.35 → 529.71 | 1.01 |
  | `save_psd` | 638.37 → 667.43 | 1.05 | 716.08 → 693.60 | 0.97 |
  | `export_float32` (f32 3000×2000) | 362.18 → 376.77 | 1.04 | 365.85 → 368.14 | 1.01 |

  (the `typed` run's figures agree: `export_png16` `ms_first` 1.31 ×.)
  No op refused at 24 MP, so every u16 consumer ran at 6000×4000.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3c-2b/…`,
  `PYTHONDONTWRITEBYTECODE=1`, run logs beside the receipts):
  `verify_phase12e2b_placement.py` (`12e2b`, load `30.57 19.59 16.92` →
  `34.70 22.49 18.15`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `31.76 22.26 18.12`) — 16 PASS;
  `verify_phase12g_clipboard.py` (`12g`, → `10.34 15.42 16.28`) — 13 PASS;
  `verify_phase12e2c_stamp_flatten.py` (`12e2c`, → `10.17 15.14 16.16`) —
  ALL GATES PASS (8 PASS); `verify_phase11c_color.py --output …/11c` (→
  `11.38 15.08 16.11`) — six of seven gates pass, C6 FAILS on its T3 half
  only (the soft proof against Adobe ACE), the known Phase 11C miss
  (`phase-11c-output-color-status.md`): the four FOGRA39 statistics equal
  the original `phase11-receipts/11c` receipt's exactly and the mask half
  passes; `verify_phase11f_float.py --output …/11f` (→ `11.19 14.98
  16.07`) — ALL GATES PASS; `verify_phase9h_ui.py --require-current
  --report 13b-3c-2b/9h.json` (→ `9.35 11.35 14.07`), screen UNLOCKED (no
  `CGSSessionScreenIsLocked` key in `ioreg`) — 56 groups + pass 2 PASS.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists ONE process,
  not this run's (pid 38071, above); left untouched. No
  `scripts/**/__pycache__`.

## Verification record (13B-3c-2c-1)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing; no other `./build/…` Rasterize instance was
running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_plates --test
  precision_projection_unseeded --test precision_projection_consumers
  --test precision_image_reads -- --nocapture --test-threads=1` — 11
  passed / 8 passed / 16 passed / 7 passed + 1 ignored. `precision_plates`
  (fixtures 600×520, 257×3, 1×1; an opaque canvas-sized u16 base and a u16
  "Off" raster at opacity 0.8, 300×200 at (−100, 400) at 600×520, partly
  off the canvas; `evict()` after the fixtures, before each measured
  call): the Current plate of either layer equals the layer placed on a
  zero canvas here, the All plate equals the render, a missing raster
  refuses "retouch requires a raster source layer"; the transformed Current
  plate at identity (alpha equal to the input's), a fractional translate,
  17° rotate and 1.5× scale; `retouch_clone` (Layer output, `source_layer`
  1), `retouch_clone` new layer, `retouch_heal` new layer (the backdrop),
  `spot_heal` and `content_aware_fill` across the (256, 256) seam — all
  publishing at 600×520, the small fixtures' answers or refusals recorded;
  Layer and Composite source selections — alpha, Rec.709 luminance and
  green equal to the written-out codes of the placed layer / the render,
  zeros outside the layer; magic wand at the seam, color range and quick
  recorded; u16 affine bilinear, perspective, resize bicubic and resize
  nearest (layer 0 equal to the written-out pixel-centre point rule) and
  f32 rotate90 / flip_horizontal (bit-equal to the written-out
  permutations); `display_rgba8`, `downconvert`, `float_to_u16`,
  `narrow_structure_to_u8` / `_u16` recorded. `no_view_retouch_plates`
  (0 process-wide with a held plate in each mode and its held transform),
  `no_view_retouch_clone_source_layer` (exactly the target layer's
  `W·H·8` — the `precision_retouch.rs:368` bind), `no_view_layer_selection`
  (0 over the document), `no_view_geometry_inputs` (affine, resize, f32
  rotate90: 0 over the source document), `seeded_outputs_keep_their_view`
  (the transformed layer's `w·h·8`; `float_to_u16` exactly `W·H·8`).
  `record_narrow_preview`'s new `narrow_preview/rgba16_fractional` lines
  (counts asserted, deltas recorded).
- HEAD comparison (`git archive 549b10e core` into a scratch directory,
  `precision_plates.rs`, `precision_projection_unseeded.rs` and the example
  copied in): at HEAD the four `no_view_*` FAIL (by design),
  `seeded_outputs_keep_their_view` passes; `record_*` HEAD vs tree — all
  108 `plates` lines and all 32 `unseeded` lines (the three new
  `rgba16_fractional` ones included) identical (answer sha256, checkpoints,
  report-log hash).
- The regression set (`precision_retouch`, `precision_inpaint`,
  `retouch_ffi`, `retouch_sampling`, `precision_retouch_progress`,
  `retouch_output`, `precision_knockout`, `precision_scalar_selection`,
  `precision_selection_refine`, `precision_geometry`, `float_exact_geometry`,
  `geometry_sets`, `retained_sources`, `develop_inspection`,
  `float_document`, `float_interchange`, `precision_conversion`,
  `precision_conversion_narrow`, `export_pipeline`,
  `precision_projection_export`) — all green.
- `make test` twice: 291 suites, 2879 passed, 0 failed, 26 ignored each
  (run 1 load `6.19 11.16 11.59`; run 2 `15.97 19.07 15.53`). No known
  load flake fired.
- `make cancel-bench` once, serial (load `17.91 20.68 16.92` → `13.78
  19.31 16.62`): 14 suites, 21 passed, 0 failed —
  `regional_group_materialization_cancel` and
  `regional_byte_preparation_cancel` passed this time.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **47 = 2
  definitions + 45 call lines** in 31 files (HEAD 55 in 36 files).
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=plates`, HEAD (the
  `549b10e` library with this example) and tree alternated, 3 samples each
  (load before each: HEAD `11.50 14.98 12.77`, tree `9.89 14.32 12.60`,
  HEAD `8.13 13.46 12.34`, tree `7.67 12.91 12.18`, HEAD `10.40 12.99
  12.22`, tree `8.21 12.27 11.99`); every `sha256` equal HEAD / tree.
  `retouch_clone_new_layer` is refused at 24 MP on both (admission needs
  3138888832 bytes, limit 2147483648) and ran at 3000×2000; every other
  u16 line ran at 6000×4000. BOUNDED medians (bound ≤ 1.10 × HEAD
  `ms_warm`, fixed before measuring; all MET, worst `select_wand` 1.04):

  | line | size | `ms_first` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio | `views_held` HEAD → tree | `views_after` HEAD → tree |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `retouch_source_current` | 6000×4000 | 70.1 → 59.2 | 0.84 | 57.8 → 48.6 | 0.84 | 240000000 → 0 | 48000000 → 0 |
  | `retouch_source_below` | 6000×4000 | 370.1 → 364.4 | 0.98 | 335.1 → 337.1 | 1.01 | 192000000 → 0 | 0 → 0 |
  | `transform_retouch_source` | 6000×4000 | 528.1 → 521.8 | 0.99 | 510.8 → 524.4 | 1.03 | 384000000 → 0 | 192000000 → 0 |
  | `retouch_clone` | 6000×4000 | 692.8 → 679.7 | 0.98 | 635.8 → 639.3 | 1.01 | 432000000 → 384000000 | 240000000 → 192000000 |
  | `retouch_clone_new_layer` | 3000×2000 | 258.7 → 266.1 | 1.03 | 256.7 → 258.5 | 1.01 | 60000000 → 60000000 | 12000000 → 12000000 |
  | `spot_heal` | 6000×4000 | 318.8 → 313.7 | 0.98 | 246.4 → 237.9 | 0.97 | 384000000 → 384000000 | 192000000 → 192000000 |
  | `select_luminance_composite` | 6000×4000 | 461.0 → 439.5 | 0.95 | 436.5 → 427.2 | 0.98 | 0 → 0 | 0 → 0 |
  | `select_luminance_layer` | 6000×4000 | 101.2 → 75.7 | 0.75 | 92.6 → 75.5 | 0.82 | 48000000 → 0 | 48000000 → 0 |
  | `select_wand` | 6000×4000 | 333.7 → 353.9 | 1.06 | 334.6 → 348.8 | 1.04 | 0 → 0 | 0 → 0 |
  | `select_color_range` | 6000×4000 | 1640.3 → 1614.4 | 0.98 | 1634.7 → 1605.0 | 0.98 | 0 → 0 | 0 → 0 |
  | `display_rgba8` | 3000×2000 | 836.8 → 795.4 | 0.95 | 820.3 → 830.2 | 1.01 | 0 → 0 | 0 → 0 |

  RECORDED without a bound:

  | line | size | `ms_first` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio | `views_held` HEAD → tree | `views_after` HEAD → tree |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `geometry_affine_bilinear` | 6000×4000 | 185.3 → 181.3 | 0.98 | 180.0 → 183.9 | 1.02 | 107564008 → 59564008 | 48000000 → 0 |
  | `geometry_resize_bicubic` | 6000×4000 | 624.4 → 564.8 | 0.90 | 582.7 → 548.0 | 0.94 | 375000000 → 135000000 | 240000000 → 0 |
  | `geometry_rotate90_float` | 3000×2000 | 66.1 → 61.3 | 0.93 | 52.4 → 63.4 | 1.21 | 192000000 → 96000000 | 96000000 → 0 |
  | `export_png16_half` | 6000×4000 | 566.0 → 529.2 | 0.94 | 537.5 → 553.7 | 1.03 | 192000000 → 0 | 192000000 → 0 |

  Tree `views_held` 0 on both `retouch_source_*` and
  `transform_retouch_source`; `views_after` 0 on `select_luminance_layer`.
  `retouch_clone`'s remaining 384000000 held are the target layer's
  `precision_retouch.rs:368` bind (192000000, 13B-3d, still live after
  the answer is freed) and the output layer's `rebuilt` seed; `spot_heal`'s
  the same pair.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3c-2c-1/…`,
  `PYTHONDONTWRITEBYTECODE=1`, run logs beside the receipts):
  `verify_phase12e2b_placement.py` (`12e2b`, load `10.78 13.85 14.78`) —
  ALL GATES PASS (13 PASS); `verify_phase12h_conversion.py` (`12h`, `15.46
  14.45 14.94`) — 16 PASS; `verify_phase12g_clipboard.py` (`12g`, `19.04
  15.25 15.22`) — 13 PASS; `verify_phase12f1_geometry.py` (`12f1`, `5.95
  10.52 13.16`) — ALL GATES PASS (13 PASS);
  `verify_phase9d_native.py --report 9d.json --require-current` (`14.80
  12.18 13.69`) — passed (10 groups at 8 and 16 bits);
  `verify_phase9g.py --report 9g.json --require-current` (`14.09 12.08
  13.65`) — FAILED before any gate: `scripts/live-filter-probe/native.py`'s
  `prefix` asserts the `.rz` version word is one of 13…24 and meets 45,
  the identity word every `.rz` has carried since 12E-1 — a stale probe
  reader, not this run's code (open item); `verify_phase11f_float.py
  --output 11f` (`14.09 12.08 13.65`) — ALL GATES PASS (11 PASS); `verify_phase9h_ui.py
  --require-current --report 9h.json` (`11.49 11.62 13.45` → `6.30 8.88
  11.72`), screen UNLOCKED (no `CGSSessionScreenIsLocked` key in `ioreg`)
  — 56 groups + pass 2 PASS.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3c-2c-2)

All run on 2026-09-25 on this machine (the user's other work running; load
recorded beside every timing; no other `./build/…` Rasterize instance was
running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_constructor_seeds --test
  precision_projection_unseeded --test precision_plates --test
  precision_image_reads -- --nocapture --test-threads=1` — 7 passed / 8
  passed / 11 passed / 7 passed + 1 ignored. `precision_constructor_seeds`
  (fixtures 600×520, 257×3, 1×1 at u16 — an opaque canvas-sized base and
  an "Off" raster partly off the canvas — and f32 — one extended-range
  raster through a float TIFF; `evict()` after the fixtures, before each
  measured call): through `rz_precision_doc_clipboard_copy` Copy of the
  u16 layer 1 / the f32 layer 0 over a window across both seams and over
  the whole canvas (partly outside the u16 layer), each with and without a
  0 / 65535 / fractional coverage, Copy Merged (seam window; whole canvas
  with coverage) and Cut (seam window, with and without coverage) at both
  depths — each stream's pixel section equal to the payload image's
  samples written out little-endian here (u16 codes, f32 bits);
  `converted` of a Display P3 u16 payload to sRGB u16 and of an f32
  payload to sRGB u16 and to u16 in its own profile (lines only);
  `convert` u16 → f32 equal to `v / 65535` and f32 → u16 equal to
  `round(clamp(v, 0, 1) · 65535)`, written out. `no_view_clipboard_payloads`
  (0 process-wide while each Copy / Copy Merged handle lives, both depths;
  a Cut leaves exactly the views over `[doc, result]`),
  `no_view_converted_input` (exactly the output's `W·H·8` after an evict,
  P3 u16 and f32 inputs), `no_view_convert` (0 while the converted image
  lives, both directions), `seeded_outputs_keep_their_view` (Rasterize's
  layer `w·h·8`; `add_layer_at` `W·H·8` u16 / `W·H·16` f32; the converted
  paste's layer `w·h·8`; `rz_artwork_render` `W·H·8` process-wide).
- `cargo test --release` of `precision_clipboard`,
  `precision_clipboard_boundaries`, `precision_clipboard_editable`,
  `precision_image_tiles`, `precision_image_accessor`,
  `precision_image_writers`, `precision_prototype`, `precision_codec`,
  `precision_insert`, `precision_paint`, `float_document`,
  `float_interchange` — all green (12 / 12 / 8 / 9 + 1 ignored / 7 / 9 / 9
  / 6 / 9 / 11 / 14 / 6).
- HEAD comparison (`git archive 71e4873 core` into a scratch directory,
  `precision_constructor_seeds.rs` and the example copied in): at HEAD the
  three `no_view_*` FAIL (by design: a held u16 Copy handle 2496000 view
  bytes; `convert` u16 → f32 4992000; `converted` 4992000 where 2496000
  is the output's), `seeded_outputs_keep_their_view` passes; `record_*`
  HEAD vs tree — all 63 `seeds` lines identical (answer sha256,
  checkpoints, report-log hash).
- `make test` twice: 292 suites, 2886 passed, 0 failed, 26 ignored each
  (run 1 load `31.14 24.95 16.40`; run 2 `20.17 37.42 25.82`). No known
  load flake fired.
- `make cancel-bench` once, serial (load `5.63 24.04 22.25` → `4.84 21.51
  21.41`): 14 suites, 21 passed, 0 failed —
  `regional_group_materialization_cancel` passed this time.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **43 = 2
  definitions + 41 call lines** in 29 files (HEAD 47 in 31 files).
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=clipboard`, HEAD (the
  `71e4873` library with this example) and tree alternated, 3 samples each
  (load before each: HEAD `25.04 22.17 14.13`, tree `21.42 21.50 14.03`,
  HEAD `17.03 20.51 13.85`, tree `14.07 19.68 13.67`, HEAD `11.91 18.93
  13.50`, tree `8.87 17.78 13.22`); every `sha256` equal HEAD / tree; no
  line was refused at 24 MP. BOUNDED medians (bound ≤ 1.10 × HEAD
  `ms_warm`, fixed before measuring): `copy_merged`, `cut_layer`,
  `paste_convert` and `copy_layer_float` MET; **`copy_layer` MISSED
  (1.157 ×)**:

  | line | size | `ms_first` HEAD → tree | ratio | `ms_warm` HEAD → tree | ratio | `views_held` HEAD → tree | `views_after` HEAD → tree |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `copy_layer` | 6000×4000 | 95.7 → 102.5 | 1.07 | 80.6 → 93.2 | **1.16** | 192000000 → 0 | 0 → 0 |
  | `copy_merged` | 6000×4000 | 445.4 → 446.2 | 1.00 | 406.2 → 407.2 | 1.00 | 192000000 → 0 | 0 → 0 |
  | `copy_layer_float` | 3000×2000 | 32.3 → 37.4 | 1.16 | 31.2 → 33.8 | 1.08 | 96000000 → 0 | 0 → 0 |
  | `cut_layer` | 6000×4000 | 231.8 → 252.7 | 1.09 | 222.4 → 227.4 | 1.02 | 576000000 → 384000000 | 192000000 → 192000000 |
  | `paste_convert` | 6000×4000 | 3915.1 → 3834.5 | 0.98 | 3814.4 → 3804.9 | 1.00 | 384000000 → 192000000 | 192000000 → 0 |

  A second alternated set, run after the drives at lower load (`5.53 5.69
  10.29` → `3.61 5.09 9.62`) and RECORDED only (the verdict above is the
  first set's): `copy_layer` warm 80.1 → 89.4 ms (1.12 ×),
  `copy_layer_float` 31.1 → 34.7 ms (1.12 ×), `copy_merged` 1.04 ×,
  `cut_layer` 1.03 ×, `paste_convert` 0.99 ×; `sha256` and view figures
  as above. The whole-layer Copy pays ~9 ms (u16 24 MP) / ~3 ms (f32
  6 MP) more warm in both sets; the cause is not measured here (the
  unseeded constructor drops the adopted buffer inside the call, where
  HEAD kept it as the handle's view until the handle was freed, and
  `encode` walks tile runs instead of one buffer) — open item.
  `cut_layer`'s remaining 384000000 held are the erase's whole-layer bind
  and the erased layer's `rebuilt` seed (13B-3d); `paste_convert`'s
  192000000 the converted layer's seed (Seeded, D2).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3c-2c-2/…`,
  `PYTHONDONTWRITEBYTECODE=1`, run logs beside the receipts):
  `verify_phase9g.py --report 9g.json --require-current` (load `3.83 10.01
  15.43`) — **passed** (`"passed": true`; 7 groups at 8 bits, 20 at 16
  bits; the repaired reader reads every capture);
  `verify_phase12e2b_placement.py` (`12e2b`, `3.88 9.19 14.85`) — ALL
  GATES PASS (13 PASS); `verify_phase12h_conversion.py` (`12h`, `9.06
  10.03 14.90`) — 16 PASS; `verify_phase12g_clipboard.py` (`12g`, `8.19
  9.81 14.77`) — 13 PASS; `verify_phase9h_ui.py --require-current --report
  9h.json` (`3.57 7.42 12.39` → `5.55 5.70 10.35`), screen UNLOCKED (no
  `CGSSessionScreenIsLocked` key in `ioreg`) — 56 groups + pass 2 PASS.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3d-1)

All run on 2026-09-25/26 on this machine (the machine heavily loaded by
other work — 1-minute load 16–154 — recorded beside every timing; no other
`./build/…` Rasterize instance of ours was running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_rect_writers --test
  precision_paint --test precision_brush_tools --test precision_healing
  --test precision_clipboard --test precision_clipboard_boundaries --test
  precision_constructor_seeds --test precision_image_writers --test
  precision_locks --test layer_raster_writers --test float_document --
  --test-threads=1` — 4 / 11 / 16 / 4 / 12 / 12 / 7 / 9 + 1 ignored / 12 /
  15 / 14 passed, 0 failed. `precision_rect_writers` sites: 7D paint,
  erase, clone (source layer 1), heal (a 64×64 window at the dab's
  origin); `paint_blend` Normal / Multiply / Dissolve, `erase_native`,
  `dodge_burn`, `bucket_fill`, `gradient`, each with and without the
  selection plane (zero, 20000 and 65535 bands); the Cut of a u16 layer
  and of an f32 layer (a float TIFF). Cases: `dab` 300×300 at (100, 100)
  and `seam` at (200, 200) on a canvas-sized 1024×768 layer,
  `corner_top_left` / `corner_bottom_right` 40×40, `one` 1×1, `edge` (a
  700×500 layer at (−50, 30) on 600×400, the dab across canvas x = 0 and
  the layer's top edge), `offcanvas` (a 300×200 layer at (1100, 50)),
  `empty` (zero coverage), `locked` (`layer_lock` 1; paint, erase), `grow`
  (across the 700×500 layer's top edge) and `inside` (the expanding tools
  only). 181 answers: 118 changed in place, 30 GROWN, 33 `null`, 0
  refusals.
- `cargo test --release --test precision_image_writers j_contiguous --
  --ignored --test-threads=1` — passed (8193² f32 contiguous: five
  `read_rect_rgba32` rects equal to (g)'s formula, zero area / leaving
  `None`; `with_rect_rgba32` unchanged `Ok(None)`, a 300×300 change `Some`
  with an untouched sample outside and `shared_tile_count` 0, `-0.0` over
  `+0.0` a change, zero area `Ok(None)`, leaving and a count mismatch
  `Err`; 0 view bytes).
- HEAD comparison (`git archive 97e5825 core scripts` into a scratch
  directory, `precision_rect_writers.rs` and the example copied in): at
  HEAD `no_view_rect_writers` FAILS (by design: `paint/dab` leaves
  12582912 view bytes — the layer's view and the rebuilt seed) and
  `dab_checkpoints_are_plane_size_independent` FAILS (by design: `paint`
  2117 checkpoints at 1024×768, 5381 at 2048×1536); the other two pass.
  All 181 `rect` lines identical HEAD / tree, every GROWN answer included.
- `make test` twice: 293 suites, 2890 passed, 0 failed, 27 ignored each
  (run 1 load `101.51 42.76 27.97`; run 2 `36.55 78.19 53.37`). No known
  load flake fired.
- `make cancel-bench` once, serial (load `23.85 49.21 46.30` before and
  after): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **38 = 2
  definitions + 36 call lines** in 26 files (HEAD 43 in 29 files).
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=dab`, HEAD (the `97e5825`
  library with this example) and tree alternated, 3 samples each (load
  before each: HEAD `97.87 32.24 23.64`, tree `154.49 45.07 28.22`, HEAD
  `144.36 44.79 28.22`, tree `135.85 44.68 28.28`, HEAD `126.17 44.18
  28.20`, tree `118.31 43.91 28.20`). Bounds FIXED before measuring, all
  MET: `precision_pixels.unique_bytes` 208777216 on both lines at HEAD and
  tree; tree `contiguous_view_bytes == contiguous_view_bytes_base`;
  `sha256` equal HEAD / tree (and equal between the two lines:
  `a47b902fb3af…`); tree `ms_median` ≤ 0.50 × HEAD (median of 3):

  | line | `ms_median` HEAD → tree (3 samples) | median of 3 | ratio | checkpoints per dab | views (base) HEAD → tree | views allocations |
  | --- | --- | --- | --- | --- | --- | --- |
  | `typed/dab` | 135.12 / 107.67 / 107.15 → 10.91 / 10.72 / 10.89 | 107.67 → 10.89 | **0.10** | 25585 → 627 | 960000000 (192000000) → 192000000 (192000000) | 5 → 1 |
  | `typed/dab_blend` | 244.54 / 237.54 / 238.98 → 11.19 / 11.10 / 11.38 | 238.98 → 11.19 | **0.05** | 18232 → 29 | 960000000 (0) → 192000000 (192000000) | 5 → 1 |

  `unique_retained_bytes` 1168820667 → 400820667 on both lines (the four
  dropped views). HEAD's `dab_blend` base figure is 0 because the paint
  chain's five views had already pushed the base's seed out of the 1 GiB
  LRU.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3d-1/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase12e2b_placement.py` (`12e2b`,
  load `24.24 30.69 38.31`) — 13 PASS; `verify_phase12h_conversion.py`
  (`12h`, `30.57 31.29 38.10`) — 16 PASS; `verify_phase12g_clipboard.py`
  (`12g`, `40.56 33.54 38.78`) — 13 PASS;
  `verify_phase13a6c_bounded_strokes.py` (`13a6c`, `58.47 72.33
  55.68`) — 4 PASS;
  `verify_phase9g.py --report 9g.json --require-current` (`47.96 69.33
  54.90`) — passed (7 + 20 groups), and again after carried nit 2
  (`9g-after-nit2.json`, `16.21 38.81 45.35`) — passed (7 + 20 groups);
  `verify_phase9h_ui.py --require-current --report 9h.json` (`36.16 63.51
  53.43`), screen UNLOCKED (no `CGSSessionScreenIsLocked` key in `ioreg`)
  — 56 groups + pass 2 PASS.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3d-2)

All run on 2026-09-26 on this machine (shared and loaded by other work —
1-minute load 7–77 — recorded beside every timing; no other `./build/…`
Rasterize instance of ours was running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_retouch_writers --test
  precision_rect_writers --test precision_plates --test precision_retouch
  --test precision_retouch_progress --test precision_healing --test
  precision_inpaint --test precision_redeye --test retouch_output --test
  content_aware_workflow --test precision_locks --test
  precision_channel_math --test precision_masks --test
  precision_scalar_selection --test selected_adjustments --test
  precision_adjustments --test precision_color --test
  precision_projection_unseeded --test precision_image_writers --test
  precision_brush_tools --test float_document -- --test-threads=1` — all 21
  suites passed (4 / 4 / 11 / 7 / 1 / 4 / 7 / 4 / 14 / 8 / 12 / 8 / 10 / 16
  / 15 / 9 / 9 / 8 / 9 + 1 ignored / 16 / 14), 0 failed.
  `precision_retouch_writers` sites (21): in place `retouch_clone` (source
  layer 1; also with the selection plane), `retouch_heal`, `spot_heal`,
  `content_aware_fill` (a ≤ 64×64 hole inside the dab, across the seam on
  `seam`), `red_eye`; `retouch_clone` new-layer and `expand_destination`;
  `scalar_edit` red paint and alpha fill (selected); `apply_mask` (a
  layer-sized mask); `apply_adjustment` invert and a `bcs` brightness 1e-7
  no-op; `selected_edit` adjustment (invert) and filter (high pass r 2);
  `convert_to_profile` (Display P3, `{}`); on a `layer_lock` 1 layer 7D
  paint, `retouch_clone`, `apply_adjustment`; the float Cut unlocked and
  locked. Cases: `dab` / `seam` 300×300 at (100, 100) / (200, 200) on a
  canvas-sized 1024×768 layer, `edge` (700×500 at (−50, 30) on 600×400),
  `offcanvas` (300×200 at (1100, 50)), `missing` (300×200 at (600, 500):
  the dab on the canvas, off the layer), `one` 1×1, `empty`. 147 answers;
  4 refusals (spot heal / fill on `offcanvas` and `missing`: "there is
  nothing to sample from …", identical at HEAD).
- HEAD comparison (`git archive a33e203 core scripts` into a scratch
  directory, the three test files and the example copied in): at HEAD
  exactly `no_view_retouch_writers` (by design: `retouch_clone/dab` leaves
  12582912 view bytes), `float_destructive_adjustment_refuses_by_name`
  ("internal error: applying high-depth adjustment" — the view `unwrap`
  panic), `precision_rect_writers::no_view_rect_writers` (`paint/locked`
  12582912) and `precision_plates::no_view_retouch_clone_source_layer`
  (2496000) fail; `record_retouch_writers` and
  `retouch_dabs_share_untouched_tiles` pass. All 147 `retouch` lines, all
  182 `rect` lines and every `plates` answer hash are identical HEAD / tree;
  the `plates` lines of the in-place retouch ops (`retouch_clone/layer`,
  `spot_heal`, `content_aware_fill` at 600×520 / 257×3 / 1×1) differ only in
  their checkpoint counts (e.g. `retouch_clone/layer` 600×520 1875 at HEAD)
  and hence the report-log hash (which carries each report's checkpoint
  count); the same logs without the counts (progress bits and stage) are
  identical HEAD / tree for all 15 `retouch/*` lines.
- `make test` twice: 294 suites, 2894 passed, 0 failed, 27 ignored each
  (run 1 load `7.03 11.92 17.82` before, `31.79 50.88 37.01` after; run 2
  `27.29 49.28 36.60` / `6.98 30.58 31.11`). No known load flake fired.
- `make cancel-bench` once, serial (load `7.89 29.61 30.76` before, `7.73
  27.19 29.83` after): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **26 = 2
  definitions + 24 call lines** in 19 files (HEAD 38 in 26 files).
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=retouch`, HEAD (the
  `a33e203` library with this example) and tree alternated, 3 samples each
  (load before each: HEAD `11.69 14.72 19.62`, tree `11.74 14.60 19.50`,
  HEAD `10.95 14.33 19.34`, tree `10.38 14.05 19.16`, HEAD `9.44 13.66
  18.93`, tree `8.59 13.27 18.69`). No line was refused at 24 MP. Bounds
  FIXED before measuring:

  | line | HEAD → tree (3 samples) | median of 3 | ratio | bound | checkpoints per dab | views (base) HEAD → tree |
  | --- | --- | --- | --- | --- | --- | --- |
  | `typed/retouch_dab` `ms_median` | 798.79 / 766.46 / 776.20 → 608.45 / 592.00 / 599.83 | 776.20 → 599.83 | **0.77** | ≤ 1.00 MET (≤ 0.75 expectation MISSED) | 39447 → 21889 | 960000000 (192000000) → 192000000 (192000000) |
  | `typed/locked_dab` `ms_median` | 80.57 / 81.81 / 78.87 → 11.12 / 11.03 / 11.16 | 80.57 → 11.12 | **0.14** | ≤ 0.50 MET | 627 → 627 | 768000000 (192000000) → 192000000 (192000000) |
  | `typed/scalar_dab` `ms_median` | 302.32 / 289.40 / 295.71 → 318.58 / 307.61 / 306.30 | 295.71 → 307.61 | **1.04** | ≤ 1.00 MISSED | 17952 → 17952 | 960000000 → 960000000 |
  | `typed/whole_writers` `apply_adjustment` `ms_warm` | 374.47 / 364.92 / 354.08 → 392.75 / 376.29 / 388.04 | 364.92 → 388.04 | **1.06** | ≤ 1.00 MISSED | — | `views_held` 384000000 → 192000000, `views_after` 192000000 → 0 |

  `precision_pixels.unique_bytes` 208777216 on the three dab lines at HEAD
  and tree (≤ 208777216 MET); tree `contiguous_view_bytes ==
  contiguous_view_bytes_base` on `retouch_dab` / `locked_dab`; `sha256`
  equal HEAD / tree on all four lines (`17fab5111efd…`, `a47b902fb3af…` —
  `typed/dab`'s, as a transparency lock on an opaque corpus changes
  nothing —, `25b62aadc29b…`, `42eeb12191b8…`). `unique_retained_bytes`
  1168820667 → 400820667 (`retouch_dab`) and 976820667 → 400820667
  (`locked_dab`). The two misses are recorded, not widened (open items,
  13A-latency).
- Review fix pass (the adjustment's changed scan and `precision_scalar_edit`'s
  `read()` in TILE-BAND order, the scalar RGBA writer as run extends + one
  flat pass), same example, HEAD / tree alternated, 3 samples each (load
  before each: HEAD `20.21 25.54 21.91`, tree `16.13 24.26 21.53`, HEAD
  `14.44 23.62 21.34`, tree `12.12 22.66 21.04`, HEAD `10.27 21.73 20.73`,
  tree `8.87 20.86 20.44`); bounds unchanged:

  | line | HEAD → tree (3 samples) | median of 3 | ratio | bound |
  | --- | --- | --- | --- | --- |
  | `typed/retouch_dab` `ms_median` | 767.45 / 770.20 / 759.44 → 592.77 / 592.29 / 594.39 | 767.45 → 592.77 | **0.77** | ≤ 1.00 MET (≤ 0.75 expectation MISSED) |
  | `typed/locked_dab` `ms_median` | 78.10 / 80.61 / 78.98 → 11.05 / 10.89 / 11.01 | 78.98 → 11.01 | **0.14** | ≤ 0.50 MET |
  | `typed/scalar_dab` `ms_median` | 292.83 / 297.09 / 291.23 → 253.92 / 247.61 / 245.59 | 292.83 → 247.61 | **0.85** | ≤ 1.00 MET |
  | `typed/whole_writers` `apply_adjustment` `ms_warm` | 352.86 / 358.64 / 353.28 → 366.25 / 361.01 / 356.34 | 353.28 → 361.01 | **1.02** | ≤ 1.00 MISSED |

  `sha256` and view bytes unchanged from the table above on all four
  lines; all 147 `retouch` / 182 `rect` record lines still equal to HEAD.
  The `apply_adjustment` miss is recorded, not widened (open item,
  13A-latency).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3d-2/…`,
  `PYTHONDONTWRITEBYTECODE=1`): `verify_phase9d_native.py --report
  9d-native.json --require-current` (load `33.93 20.72 24.44`) — passed;
  `verify_phase12e1b_structure.py` (`12e1b`, `36.93 22.28 24.92`) — 8 PASS;
  `verify_phase11f_float.py` (`11f`, `37.10 22.56 25.00`) — 11 PASS;
  `verify_phase12e2b_placement.py` (`12e2b`, `43.42 24.11 25.53`) — 13
  PASS; `verify_phase12h_conversion.py` (`12h`, `45.20 27.19 26.61`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, `41.03 26.90 26.51`) — 13
  PASS; `verify_phase9h_ui.py --require-current --report 9h.json` (`6.90
  15.50 21.50`) — 55 groups PASS, `current-layer-commands-ui` FAIL ("the owned editor never
  became the real key window"), and again alone
  (`RZ_PAINT_UI_CASES=current-layer-commands-ui`, `9h-current-layer-
  commands.json`, `4.93 9.43 16.87`) the same FAIL: the screen was LOCKED
  (`CGSSessionScreenIsLocked = true` in `ioreg`), the known lock-state
  failure, not a code failure.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3d-3a)

All run on 2026-09-26 on this machine (shared and loaded by other work —
1-minute load 6–42 — recorded beside every timing; no other `./build/…`
Rasterize instance of ours was running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_layer_producers --test
  precision_image_writers --test precision_retouch_writers --test
  precision_rect_writers --test precision_projection_unseeded --test
  precision_constructor_seeds --test precision_plates --test precision_merge
  --test precision_stamp_flatten --test precision_knockout --test
  knockout_merge --test persisted_warp_native --test persisted_warp_numeric
  --test precision_paint --test precision_clipboard --test float_document
  --test precision_conversion_narrow --test conversion_policy_tests --test
  precision_color --test precision_retouch --test precision_brush_tools
  --test selected_adjustments --test precision_adjustments --
  --test-threads=1` (load `10.52 17.52 16.88` → `17.39 18.41 17.21`) — all
  23 suites passed, 0 failed (1 ignored: `precision_image_writers` (j)).
  `precision_layer_producers` sites (18 site / depth pairs × 600×520,
  257×3, 1×1): `merge_layers` / `merge_down` / `merge_visible` /
  `stamp_visible` / `flatten` at u16 and f32 (the f32 document a float base
  plus a seam window of a second float document pasted in place), `bake`,
  `warp` (3×3 bilinear, non-translation), `artwork`, `rasterize`,
  `paste_convert`, `converted`, `float_to_u16`, `narrow_to_u16`.
- HEAD comparison (`git archive 63421b3 core scripts` into a scratch
  directory, the new suite, the six edited test files and the example
  copied in; nothing in the working tree touched): at HEAD exactly the
  tightened assertions fail — `precision_layer_producers::no_view_layer_producers`
  (every arm: u16 merges / stamp / flatten / artwork / float_to_u16 2496000,
  f32 merges / stamp / flatten 4992000, bake 4992000, rasterize 480000,
  paste_convert / converted 1497600, narrow_to_u16 3993600, warp 5005776 =
  the source's 2496000 + the output's 2509776),
  `precision_image_writers` (a) / (b) / (c),
  `precision_retouch_writers::no_view_retouch_writers`
  (`retouch_clone/new_layer/dab` 6291456),
  `precision_rect_writers::no_view_rect_writers` (`paint_blend_normal/edge`
  2968000, a GROWN answer) and
  `precision_constructor_seeds::no_view_converted_input` (2496000); every
  other test passes there. All 586 record lines — 54 `producer`, 147
  `retouch`, 182 `rect`, 26 `unseeded`, 63 `seeds`, 114 `plates` — are
  identical HEAD / tree (answers, checkpoint counts and report-log hashes).
- `make test` twice: 295 suites, 2896 passed, 0 failed, 27 ignored each
  (run 1 load `6.42 12.32 15.03` before, `27.98 39.04 28.31` after; run 2
  `27.98 39.04 28.31` / `6.42 23.76 23.88`). No known load flake fired.
- `make cancel-bench` once, serial (load `6.42 23.76 23.88` before, `5.84
  21.45 23.02` after): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **26 = 2
  definitions + 24 call lines** in 19 files (unchanged). Seeded-site count
  (the constructor / `OutputView::Seeded` `rg` above): **47 → 35** lines.
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=producers` and
  `RZ_TILE_SHARING_ONLY=retouch`, HEAD (the `63421b3` library with this
  example) and tree alternated, 3 samples each (load before each —
  producers: HEAD `15.17 17.84 17.03`, tree `23.73 20.63 18.16`, HEAD
  `13.93 18.52 17.51`, tree `10.46 16.91 16.96`, HEAD `10.66 16.14 16.69`,
  tree `14.53 16.22 16.68`; retouch: HEAD `11.42 15.30 16.32`, tree `9.70
  14.73 16.10`, HEAD `8.97 14.41 15.97`, tree `7.73 13.86 15.75`, HEAD
  `8.39 13.80 15.71`, tree `7.44 13.34 15.51`). No line was refused at its
  first size. Bounds FIXED before measuring (spec step 8):

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- |
  | `merge_down` | 24 MP | 351.34 / 330.04 / 330.72 → 333.42 / 345.86 / 341.40 | 330.72 → 341.40 | 1.03 | 192000000 → 0 | MET |
  | `merge_visible` | 24 MP | 332.44 / 328.51 / 333.74 → 337.02 / 336.59 / 339.08 | 332.44 → 337.02 | 1.01 | 192000000 → 0 | MET |
  | `stamp_visible` | 24 MP | 336.21 / 332.44 / 337.22 → 330.63 / 335.10 / 324.98 | 336.21 → 330.63 | 0.98 | 192000000 → 0 | MET |
  | `flatten` | 24 MP | 333.21 / 329.46 / 339.73 → 336.14 / 320.33 / 328.31 | 333.21 → 328.31 | 0.99 | 192000000 → 0 | MET |
  | `rasterize` | 24 MP | 319.07 / 366.71 / 407.85 → 359.87 / 363.08 / 336.01 | 366.71 → 359.87 | 0.98 | 192000000 → 0 | MET |
  | `artwork` | 24 MP | 302.88 / 300.69 / 322.44 → 303.54 / 300.19 / 302.66 | 302.88 → 302.66 | 1.00 | 192000000 → 0 | MET |
  | `bake` | 24 MP | 9798.87 / 9679.44 / 9835.84 → 9416.42 / 9400.37 / 9160.48 | 9798.87 → 9400.37 | 0.96 | 384000000 → 0 | MET |
  | `warp` | 3000×2000 | 694.48 / 711.24 / 710.59 → 692.72 / 694.09 / 691.61 | 710.59 → 692.72 | 0.97 | 96064016 → 48000000 (= the source's `w·h·8`; HEAD − the 3001×2002 output's 48064016) | MET |
  | `float_to_u16` | 3000×2000 | 881.06 / 890.15 / 912.27 → 855.19 / 865.08 / 815.92 | 890.15 → 855.19 | 0.96 | 48000000 → 0 | MET |
  | `narrow_to_u16` | 3000×2000 | 855.69 / 838.48 / 799.30 → 790.91 / 763.11 / 829.57 | 838.48 → 790.91 | 0.94 | 48000000 → 0 | MET |

  `typed/producer_consumers` (flatten, then the consumer with no eviction
  in between): `warp_after_flatten` (3000×2000) `ms_first` 704.42 / 707.59
  / 719.56 → 705.56 / 704.91 / 698.58 (median 707.59 → 704.91, **1.00 ×**,
  bound ≤ 1.10 × MET), `ms_warm` 714.94 / 700.59 / 719.64 → 689.75 /
  689.35 / 690.32 (714.94 → 689.75, **0.96 ×**, ≤ 1.05 × MET),
  `views_before` 48000000 → 0, `views_held` 96064016 → 48000000;
  `save_after_flatten` (24 MP, RECORDED — 13B-3e owns the save's read)
  `ms_first` 108.66 / 98.28 / 126.37 → 119.27 / 179.53 / 189.03 (108.66 →
  179.53, 1.65 ×), `ms_warm` 97.10 / 97.88 / 111.13 → 120.21 / 109.71 /
  109.04 (97.88 → 109.71, 1.12 ×), `views_before` 192000000 → 0,
  `views_held` 192000000 → 192000000 (the save's own materialization).
  Retouch lines: `typed/whole_writers` `apply_adjustment` `ms_warm` 362.73
  / 359.31 / 361.30 → 363.13 / 360.13 / 360.13 (361.30 → 360.13, **1.00
  ×**, ≤ 1.05 × MET; `views_held` 192000000 → 0); `typed/scalar_dab`
  `ms_median` 249.45 / 249.70 / 252.94 → 245.75 / 249.88 / 245.33 (249.70 →
  245.75, **0.98 ×**, ≤ 1.05 × MET; `contiguous_view_bytes` 960000000 →
  192000000 = the base's seed); recorded: `typed/retouch_dab` 592.22 →
  585.70 ms (0.99 ×), `typed/locked_dab` 11.16 → 10.82 ms (0.97 ×), both
  192000000 = the base's seed at HEAD and tree. Every `sha256` identical
  HEAD / tree on all 16 lines.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3d-3a/…`, run logs
  beside them, `PYTHONDONTWRITEBYTECODE=1`, the app BINARY path):
  `verify_phase12e2b_placement.py` (`12e2b`, load `5.40 9.66 16.06` →
  `5.69 9.26 15.62`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `5.55 9.17 15.56`) — 16 PASS;
  `verify_phase12g_clipboard.py` (`12g`, → `5.21 6.92 12.90`) — 13 PASS;
  `verify_phase12e1b_structure.py` (`12e1b`, → `5.48 6.92 12.83`) — ALL
  GATES PASS (8 PASS); `verify_phase12e2c_stamp_flatten.py` (`12e2c`, →
  `5.99 6.99 12.78`) — ALL GATES PASS (8 PASS);
  `verify_phase12f3_warp.py` (`12f3`, → `5.91 6.96 12.74`) — ALL GATES
  PASS (11); `verify_phase11f_float.py --output …/11f` (→ `5.68 6.89
  12.68`) — ALL GATES PASS (11); `verify_phase11c_color.py --output …/11c`
  (→ `14.48 8.67 13.17`) — six of seven gates pass, C6 FAILS on its T3 half
  only (the soft proof against Adobe ACE), the known Phase 11C miss: its
  T3 block equals 13B-3c-2b's receipt exactly and the mask half passes
  (8249 / 8249 at both intents); `verify_phase9d_native.py --require-current
  --report …/9d-native.json` (→ `13.39 8.63 13.11`) — passed;
  `verify_phase9h_ui.py --require-current --report …/9h.json` (→ `7.55
  8.98 12.08`) — every case PASS but `current-layer-commands-ui` ("the owned
  editor never became the real key window"), and again alone
  (`RZ_PAINT_UI_CASES=current-layer-commands-ui`,
  `9h-current-layer-commands.json`, `5.81 8.32 11.67` → `8.53 8.08 11.08`)
  the same FAIL: the screen was LOCKED (`CGSSessionScreenIsLocked = true`
  in `ioreg`, `9h-lockstate.txt`), the known lock-state failure, not a code
  failure (the probe run logs are copied beside the receipts). No owned
  drive: no app-visible behaviour changes.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3d-3b)

All run on 2026-09-26 on this machine (shared and loaded by other work —
1-minute load 4–143 — recorded beside every timing; no other `./build/…`
Rasterize instance of ours was running).

- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_filled_outputs --test
  precision_constructor_seeds --test precision_plates --test
  precision_image_writers --test precision_add_layer --test
  precision_insert --test precision_conversion_structure --test
  precision_conversion_narrow --test precision_geometry --test
  float_exact_geometry --test retained_sources --test retained_geometry
  --test develop_inspection --test precision_codec --test
  precision_projection_export --test export_pipeline --test
  precision_stamp_flatten --test precision_layer_producers --test
  precision_projection_unseeded -- --test-threads=1 --nocapture` (load
  `15.51 13.36 12.53` → `30.70 17.33 14.00`) — all 19 suites passed, 0
  failed (1 ignored: `precision_image_writers` (j)).
  `precision_filled_outputs` sites (23 site / depth pairs × 600×520,
  512×512, 257×3, 1×1): `add_layer` u16 / f32, `promote_plan` (a byte
  document with a placed carried source; the plan JSON with its `id` keys
  removed), `narrow_masked_adjustment`, `geometry_affine` / `_resize` /
  `_perspective` u16, `geometry_rotate90` / `_flip` f32,
  `placed_source_affine` (place, then a non-shared bilinear affine through
  `rz_precision_doc_source_edit`), `develop_detail`, the eight 16-bit
  exports (PNG / TIFF × plain / resize / sharpen / both, over a supplied
  projection), `encode_native_after_flatten` (the stream with its trailing
  `LIDS` ids zeroed), `codec_save_png` / `_tiff` and
  `codec_encode_refusal` (f32).
- HEAD comparison (`git archive e250ba6 core scripts` into a scratch
  directory, the new suite, the three edited test files and the example
  copied in; nothing in the working tree touched; the same 19 suites plus
  `precision_retouch_writers` and `precision_rect_writers`): at HEAD
  exactly the four by-design tests fail —
  `precision_filled_outputs::no_view_filled_planes` (u16 add_layer 2496000
  view bytes), `filled_planes_share_one_tile_per_shape` (u16 600×520
  `precision_pixels` 2496000, by hand 726528), `no_view_resample_outputs`
  (`geometry_affine` 599608) and `no_view_encode` (`encode_native` 2496000);
  every other test passes there. All 678 record lines — 92 `filled`, 54
  `producer`, 147 `retouch`, 182 `rect`, 26 `unseeded`, 63 `seeds`, 114
  `plates` — are identical HEAD / tree (answers, checkpoint counts and
  report-log hashes).
- `make test` twice: 296 suites, 2901 passed, 0 failed, 27 ignored each
  (run 1 load `5.47 6.38 12.76` before, `20.53 54.38 39.35` after; run 2
  `20.53 54.38 39.35` / `15.80 35.17 33.96`). No known load flake fired.
- `make cancel-bench` once, serial (load `15.80 35.17 33.96` before,
  `11.59 31.97 32.83` after): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **25 = 2
  definitions + 23 call lines** in 18 files (was 26). Seeded-site count
  (13B-3d-3a's `rg`): **35 → 33** lines.
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED.
- `phase13b_tile_sharing`, the example built against the tree, `e250ba6`
  and `63421b3` (`producers` only on the last), alternated, 3 samples each,
  bounds FIXED before measuring (spec step 7). Load before each run —
  `producers`: `e250ba6` `84.78 59.71 32.84` / `14.43 41.18 29.37` / `5.33
  28.21 25.84`, `63421b3` `52.06 54.31 32.22` / `10.58 36.82 28.31` / `4.31
  24.77 24.69`, tree `26.96 47.35 30.79` / `7.08 32.21 27.05` / `4.95 22.11
  23.72`; `filled`: `e250ba6` `4.06 19.76 22.79` / `3.90 19.21 22.55` / `3.45
  18.61 22.30`, tree `3.98 19.48 22.67` / `3.67 18.90 22.43` / `3.82 18.43
  22.22`; `plates`: `e250ba6` `3.83 18.19 22.11` / `4.05 16.06
  21.10` / `4.61 14.34 20.17`, tree `4.27 17.14 21.62` / `4.19 15.12 20.61`
  / `9.80 14.91 20.19`; `unseeded`: 6.2–8.0 both.

  | line | size | HEAD → tree (3 samples) | median | ratio | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- |
  | `save_after_flatten` `ms_first` | 24 MP | `63421b3` 122.80 / 103.99 / 113.82 → 103.50 / 118.20 / 148.52 | 113.82 → 118.20 | 1.04 | 192000000 → 0 | MET (binding) |
  | `save_after_flatten` `ms_warm` | 24 MP | `e250ba6` 113.26 / 110.31 / 115.44 → 103.14 / 123.17 / 119.71 | 113.26 → 119.71 | 1.06 | 192000000 → 0 | **MISSED** (≤ 1.05) |
  | `add_layer` u16 `ms_warm` | 24 MP | 19.41 / 17.69 / 19.12 → 0.0198 / 0.0175 / 0.0180 | 19.12 → 0.018 | 0.001 | 192000000 → 0 | MET |
  | `add_layer` f32 `ms_warm` | 3000×2000 | 16.45 / 18.93 / 19.11 → 0.0622 / 0.0583 / 0.0625 | 18.93 → 0.062 | 0.003 | 96000000 → 0 | MET |
  | `geometry_affine_bilinear` `ms_warm` | 24 MP | 176.24 / 173.61 / 178.16 → 172.20 / 180.42 / 171.09 | 176.24 → 172.20 | 0.98 | 59564008 → 0 | MET |
  | `geometry_resize_bicubic` `ms_warm` | 24 MP | 491.85 / 494.26 / 498.87 → 490.70 / 501.81 / 496.61 | 494.26 → 496.61 | 1.00 | 135000000 → 0 | MET |
  | `geometry_rotate90_float` `ms_warm` | 3000×2000 | 57.96 / 60.48 / 60.51 → 62.10 / 60.49 / 62.92 | 60.48 → 62.10 | 1.03 | 96000000 → 0 | MET |

  `save_after_flatten`'s `sha256` is identical in all three builds and
  `views_before` is 0 at `e250ba6` and tree (192000000 at `63421b3`). The
  `ms_warm` MISS is recorded with its load: the first sample of each build
  ran at a 1-minute load of 27–85 while it fell to ~5 by the third, and the
  tree's three samples spread 103–123 ms. Cause of any real difference:
  HEAD's warm save reads the contiguous view its first save cached, the
  tree walks the tiles' runs on every save (24 runs per row). A second set
  at a steady load of 4–9 (recorded only, NOT the bound; `e250ba6` /
  `63421b3` / tree alternated): `ms_first` 121.59 / 163.32 / 133.20 /
  140.98 / 129.75 / 134.90 → tree 127.51 / 119.41 / 121.91 (medians
  `e250ba6` 133.20, `63421b3` 134.90, tree 121.91: 0.90 × `63421b3`);
  `ms_warm` `e250ba6` 103.73 / 118.46 / 124.60 → tree 114.85 / 100.98 /
  101.63 (118.46 → 101.63, 0.86 ×). The fix needs nothing beyond the run
  walk, so the save stays in this slice (no 13B-3d-3c row).
  `typed/filled` `add_layer` `layer_plane_bytes` (the plane:
  `precision_pixels` + `tile_structure`) **1227776** (u16, HEAD 192003072)
  and **3267328** (f32, HEAD 96000768) — exactly the spec's figures;
  `layer_retained_bytes` (`retained_bytes()`, which also counts the sRGB
  profile's 9121 bytes) 1236897 / 3276449 (HEAD 192012193 / 96009889).
  Recorded only: `narrow_masked_adjustment` 24 MP `ms_warm` 82.43 / 80.58
  / 82.59 → 71.76 / 77.23 / 73.01 (82.43 → 73.01, 0.89 ×), `views_held`
  192000000 → 96000000 (the base raster's `:233` seed, as expected),
  `layer_pixels` 192000000 → 96612352; `warp_after_flatten` (3000×2000)
  `ms_first` `63421b3` 687.83 → 693.78, `ms_warm` `e250ba6` 684.69 →
  687.71, `views_held` 48000000 at `e250ba6` and tree; `typed/unseeded`
  `export_png16` `ms_first` 136.26 → 116.82 (0.86 ×), `ms_warm` 96.24 →
  104.19 (1.08 ×), `contiguous_view_bytes_after_first` 384000000 →
  192000000; `export_png16_half` `ms_warm` 466.65 → 467.16 (1.00 ×). Every
  `sha256` identical HEAD / tree on every `producers`, `filled`, `plates`
  and `unseeded` line; the other `producers` lines hold HEAD's views
  (0; the warp its source's 48000000).
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3d-3b/…`, run logs
  beside them, `PYTHONDONTWRITEBYTECODE=1`, the app BINARY path):
  `verify_phase12e2b_placement.py` (`12e2b`, load `16.30 20.36 27.08` →
  `36.58 25.20 28.55`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `33.08 24.83 28.38`) — 16
  PASS; `verify_phase12g_clipboard.py` (`12g`, → `12.10 15.72 23.30`) — 13
  PASS; `verify_phase12e1b_structure.py` (`12e1b`, → `11.23 15.42 23.10`)
  — ALL GATES PASS (8 PASS); `verify_phase12f1_geometry.py` (`12f1`, →
  `11.38 15.31 22.97`) — ALL GATES PASS (13 PASS);
  `verify_phase12f2_geometry.py` (`12f2`, → `10.95 15.15 22.87`) — 11 PASS,
  0 FAIL; `verify_phase11f_float.py --output …/11f` (→ `9.68 14.67 22.57`)
  — ALL GATES PASS (11); `verify_phase9d_native.py --require-current
  --report …/9d-native.json` (→ `9.23 14.49 22.46`) — passed;
  `verify_phase9h_ui.py --require-current --report …/9h.json` (→ `8.29
  10.35 18.09`) — 55 cases PASS, `current-layer-commands-ui` FAIL ("the
  owned editor never became the real key window"), and again alone
  (`RZ_PAINT_UI_CASES=current-layer-commands-ui`, `7.94 10.24 18.00` →
  `4.13 8.06 16.08`) the same FAIL: the screen was LOCKED
  (`CGSSessionScreenIsLocked = Yes` in `ioreg`, `9h-lockstate.txt`), the
  known lock-state failure, not a code failure (both probe run logs copied
  beside the receipts). 12e3c not run. No owned drive: no app-visible
  behaviour changes. The 12h / 12g / 12f2 drives print one `PASS …` line
  per gate and no `ALL GATES PASS` summary, so that marker counts 0 by
  design (`ALL_GATES_PASS=0` beside `FAIL=0`); their verdict is the PASS
  count with no FAIL.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-1)

All run on 2026-09-26 on this machine (shared and loaded by other work —
1-minute load 3–284 — recorded beside every timing; no other `./build/…`
Rasterize instance of ours was running). HEAD comparisons ran in a scratch
copy (`git archive 2c04c47 core scripts`, the new suite, the edited
`precision_filled_outputs` and the example copied in); nothing in the
working tree was stashed or checked out.

- Baseline at `2c04c47`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 25
  in 18 files; the seeded-site `rg` of the 13B-3d-3a contract = 33 lines.
- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_export_reads --test
  precision_filled_outputs --test precision_projection_unseeded --test
  export_pipeline --test export_production --test export_host_formats
  --test precision_projection_export --test float_interchange --test
  float_native --test precision_ffi --test precision_metadata_bridge --test
  psd_depth16 --test psd_export_corpus --test precision_layer_styles --test
  precision_knockout_format --test layer_label --test sharpen_filters --
  --test-threads=1 --nocapture` (load `11.92 14.43 23.34` → `15.90 15.16
  23.49`) — 17 suites, 128 passed, 0 failed.
- HEAD comparison: at `2c04c47` exactly `no_view_export_reads` fails
  (`encoded/png16_sharpen: no view held`, 2496000 view bytes, by design);
  `record_export_reads`, `trimmed_layer_export_is_the_canvas_export_cropped`,
  `psd_records_hold_the_source_samples`, `float_exports_round_trip_exactly`
  and all 7 `precision_filled_outputs` tests (with nit 2's oracle) pass
  there. All 190 record lines — 72 `export`, 92 `filled`, 26 `unseeded` —
  are identical HEAD / tree (answers, checkpoint counts and report-log
  hashes), re-checked after each fix pass.
- `make test` twice on the final tree: 297 binaries, 2906 passed, 0 failed,
  27 ignored each (run 1 load `25.36 17.13 24.14` → `11.80 24.85 27.23`, run
  2 → `8.07 17.61 23.80`). An earlier `make test` on the first cut stopped
  at `bayesian_matting::live_cancellation_stops_both_joint_work_and_fully_explicit_copy`
  ("operation completed before active cancellation witness", load ~41–130)
  — a load race in a matting suite this run does not touch; it passed 3 of
  3 alone and in the two full runs after; recorded, not chased.
- `make cancel-bench` once, serial (load `6.98 17.06 23.53` → `5.06 15.30
  22.59`): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **17 = 2
  definitions + 15 call lines** in 13 files (was 25 in 18). Seeded-site
  count (13B-3d-3a's `rg`): **33 → 24** lines.
- `make typecheck` — 0 errors; `make app` — BUILD SUCCEEDED (load `3.63
  10.17 19.08`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=exports`, tree and HEAD
  alternated, 3 samples each, bounds FIXED before measuring (spec step 6:
  `views_held` 0 binding; `ms_warm` median ≤ 1.05 × HEAD; `ms_first`
  recorded). Load before each run: tree `33.96 23.13 29.60` / `23.98 22.70
  29.07` / `12.33 19.70 27.56`, HEAD `32.10 23.97 29.70` / `17.04 21.14
  28.29` / `11.10 18.83 27.02`. `export_png16_sharpen` and
  `export_layer_trimmed` are refused at 24 MP by admission (3257477080 /
  2296421556 bytes > 2147483648) and run at 3000×2000 in both builds.

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `export_png16_sharpen` | 3000×2000 | 278.33 / 253.86 / 239.15 → 242.73 / 230.19 / 234.74 | 253.86 → 234.74 | 0.92 | 257.08 → 240.96 | 48000000 → 0 | MET |
  | `export_tiff16_p3` | 24 MP | 4236.53 / 4252.05 / 4338.26 → 4302.27 / 4396.77 / 4257.54 | 4252.05 → 4302.27 | 1.01 | 4314.22 → 4360.66 | 192000000 → 0 | MET |
  | `export_png16_render` | 24 MP | 449.09 / 505.98 / 468.21 → 443.82 / 454.45 / 450.20 | 468.21 → 450.20 | 0.96 | 459.09 → 466.34 | 0 → 0 | MET |
  | `export_layer_trimmed` | 3000×2000 | 42.48 / 43.30 / 42.10 → 39.41 / 40.47 / 39.14 | 42.48 → 39.41 | 0.93 | 43.73 → 40.05 | 0 → 0 | MET |
  | `export16_png` | 24 MP | 528.75 / 528.60 / 529.90 → 777.51 / 537.02 / 531.47 | 528.75 → 537.02 | 1.02 | 520.28 → 543.20 | 0 → 0 | MET |
  | `save_psd_flattened` | 24 MP | 831.04 / 932.03 / 902.59 → 958.09 / 977.91 / 977.31 | 902.59 → 977.31 | 1.08 | 845.23 → 834.07 | 192000000 → 0 | **MISSED** |
  | `save_psd_layers_baked` | 24 MP | 1252.75 / 1160.79 / 1274.52 → 1116.01 / 1098.57 / 1299.34 | 1252.75 → 1116.01 | 0.89 | 1053.01 → 1058.70 | 384000000 → 0 | MET |
  | `export_float32` | 3000×2000 | 370.06 / 357.75 / 383.68 → 368.42 / 356.14 / 379.56 | 370.06 → 368.42 | 1.00 | 382.06 → 366.55 | 96000000 → 0 | MET |
  | `export_float_png16` | 3000×2000 | 875.76 / 829.66 / 804.67 → 848.84 / 868.43 / 874.30 | 829.66 → 868.43 | 1.05 | 831.79 → 857.79 | 0 → 0 | MET |
  | `save_native_float` | 3000×2000 | 18.12 / 13.70 / 12.26 → 15.56 / 17.48 / 17.84 | 13.70 → 17.48 | 1.28 | 30.12 → 15.99 | 96000000 → 0 | **MISSED** |

  Every `sha256` is identical HEAD / tree on every line and every
  `views_after` is 0 in the tree. The two MISSES are recorded with their
  load (above; the load fell from ~34 to ~11 across the six runs): HEAD's
  warm call reads the contiguous view its FIRST call cached on the
  long-lived projection / layer, while the tree walks tile runs on every
  call (HEAD's `ms_first`, which pays that materialization, is 845.23 /
  30.12 against the tree's 834.07 / 15.99). Diagnostic probes (recorded
  only, not the bound; 15 saves / 120 native encodes per process at load
  ~8–40), HEAD → tree: a 24 MP flattened save median 868 → 880 ms after
  D5 (the channel extraction ~760 → ~840 profile samples, the composite at
  parity), the 6 MP float native encode 12.5–13.4 → 14.9–15.8 ms (the
  staging fill from 4 KiB tile runs; D6's chunking did not move it). Two
  fix passes preceded the final set (recorded only): first cut
  `save_psd_flattened` 1.34 ×, `save_psd_layers_baked` 1.17 ×,
  `export_layer_trimmed` 1.08 ×, `export16_png` 1.05 ×,
  `save_native_float` 1.28 × (load ~4–8); after the `plane16` fold and the
  run-extreme scan 1.10 × / 1.06 × / 0.95 × / 1.04 × / 1.27 × (load
  ~10–43).
- `RZ_TILE_SHARING_ONLY=unseeded`, tree then HEAD (load `8.54 17.19 26.04`
  / `11.25 17.42 25.97`): every `sha256` identical on all 7 lines;
  `contiguous_view_bytes_after_first` (process-wide, inputs' seeds
  included) `save_psd` 384000000 → 192000000, `export_float32` 192000000 →
  96000000, the rest unchanged.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-1/…`, run logs in
  `logs/`, `PYTHONDONTWRITEBYTECODE=1`, the app BINARY path):
  `verify_phase12e2b_placement.py` (`12e2b`, load `13.56 12.50 19.22` →
  `10.65 11.99 18.69`) — ALL GATES PASS (13 PASS);
  `verify_phase12h_conversion.py` (`12h`, → `9.71 11.74 18.53`) — 16 PASS,
  0 FAIL; `verify_phase12g_clipboard.py` (`12g`, → `6.14 8.65 15.38`) — 13
  PASS, 0 FAIL; `verify_phase11a_export.py --output …/11a` (→ `6.21 8.62
  15.33`) — all 7 gates passed, `error` null;
  `verify_phase11d_formats.py --output …/11d` (→ `8.04 8.92 15.28`) — all
  7 gates passed, `error` null; `verify_phase11f_float.py --output …/11f`
  (→ `8.68 9.04 15.29`) — ALL GATES PASS (11 PASS);
  `verify_phase9d_native.py --require-current --report …/9d-native.json`
  (→ `9.03 9.10 15.23`) — passed; `verify_phase12e3c_psd.py --app …
  --receipts …/12e3c --skip-photoshop` (→ `13.67 10.10 15.37`) — C1–C3
  PASS, P16 ten MATCH, PD 8 / 16 DIVERGENCE (disclosed), no FAIL, the
  Photoshop gates NOT RUN by flag, so the drive reports INCOMPLETE (exit
  2) by design; `verify_phase9h_ui.py --require-current --report …/9h.json`
  (`13.56 10.32 15.33` → `5.13 7.36 12.74`) — 54 cases PASS,
  `current-layer-commands-ui` FAIL ("A clipboard key equivalent has no owned
  key editor window") and `byte-quick-mask-record-ui` FAIL
  ("paint.pressureOpacity control unavailable at byte depth"); each re-run
  alone (`RZ_PAINT_UI_CASES=…`, load `4.57 7.06 12.48` / `3.80 6.29
  11.78`) PASSED. The screen was UNLOCKED (no `CGSSessionScreenIsLocked`
  key in `ioreg`, `9h-lockstate.txt`) while the machine was in use; the
  filtered re-runs exit 1 only on `--require-current`'s "required cases
  did not run" (the filter) — both probe run logs copied beside the
  receipts. No owned drive: no app-visible behaviour changes.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-2)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load 4–72, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a scratch copy (`git archive 3f7e2e5 core scripts`, with the new suite and
the example copied in). Nothing in the working tree was stashed or checked
out.

- Baseline at `3f7e2e5`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 17
  in 13 files; the seeded-site `rg` of the 13B-3d-3a contract = 24 lines.
- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test live_filter_tile_reads --test
  live_filter_numeric --test live_filter_native --test
  live_filter_stack_numeric --test live_filter_stack_native --test
  live_filter_rank_numeric --test live_filter_denoise_numeric --test
  live_filter_neighborhood --test sharpen_filters --test cleanup_filters
  --test retained_sources --test retained_source_work --test
  layer_raster_assets --test layer_raster_transforms --test source_locks
  --test conversion_policy_tests --test develop_inspection --test
  precision_export_reads -- --test-threads=1 --nocapture` (load `23.62
  30.88 19.75` → `28.04 31.14 20.29`): 18 suites, 157 passed, 0 failed.
- HEAD comparison. At `3f7e2e5` exactly `no_view_live_filter_reads` fails,
  by design (`edit/u16: 4992000 > one filtered plane`, i.e. two planes, the
  coloured input's view and the filtered seed); `record_live_filter_reads`,
  `tiled_edit_matches_the_double_reference` and
  `preview_reads_the_cell_centres` pass there. All 44 `live` record lines
  are identical between HEAD and the tree (answers, checkpoint counts and
  report-log hashes). This was checked on the first cut, after the fix
  pass, and after the D5 revert.
- `make test` twice on the final tree: 298 binaries, 2910 passed, 0 failed,
  27 ignored each (run 1 load `21.63 29.48 19.94` → `71.73 53.61 34.50`, run
  2 → `11.28 33.19 29.53`). No known flake recurred.
- `make cancel-bench` once, serial (load `11.10 32.79 29.40` → `8.70 29.43
  28.33`): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **15 = 2
  definitions + 13 call lines** in 11 files (was 17 in 13; the spec's 14 in
  10 assumed the preview read moved, D5). Seeded-site count: **24 → 22**
  lines (spec 21; `live_filter_evaluate.rs:423` kept, D5).
- `make typecheck` — 0 errors (load `8.78 28.77 28.11`); `make app` — BUILD
  SUCCEEDED (load `3.91 18.06 23.76` → `30.00 23.19 25.37`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=live_filter`, HEAD and tree
  alternated, 3 samples each. The bounds were FIXED before measuring (spec
  Constraints): `views_held` 0, binding; `sha256` equal; `ms_warm` median ≤
  1.05 × HEAD; `ms_first` recorded with no bound. Load before each run of
  the FINAL set: HEAD `12.80 12.52 10.03` / `10.04 11.91 9.88` / `7.67 11.25
  9.69`, tree `11.51 12.25 9.97` / `8.80 11.59 9.78` / `6.95 10.98 9.61`.

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `edit_u16` | 3000×2000 | 91.90 / 91.60 / 90.52 → 89.95 / 91.51 / 89.54 | 91.60 → 89.95 | 0.98 | 96.90 → 92.71 | 96000000 → 48000000 | warm MET; views D5 residual |
  | `edit_u16_p3` | 3000×2000 | 1026.34 / 1010.48 / 968.15 → 1037.02 / 1002.97 / 992.70 | 1010.48 → 1002.97 | 0.99 | 1022.58 → 998.80 | 96000000 → 48000000 | warm MET; views D5 residual |
  | `edit_u8_master` | 3000×2000 | 105.37 / 108.06 / 103.97 → 105.44 / 102.76 / 106.24 | 105.37 → 105.44 | 1.00 | 106.57 → 105.42 | 48000000 → 48000000 | warm MET; views D5 residual |
  | `edit_byte_doc_u16` | 3000×2000 | 108.61 / 109.27 / 105.81 → 103.28 / 102.16 / 103.68 | 108.61 → 103.28 | 0.95 | 111.51 → 103.16 | 72000000 → 0 | MET |
  | `preview_before_p3` | 2000×1500 | 464.91 / 468.62 / 459.18 → 472.65 / 464.29 / 467.22 | 464.91 → 467.22 | 1.005 | 462.95 → 471.14 | 24000000 → 0 | MET (after the D6 fix pass) |
  | `preview_after` | 2000×1500 | 1.67 / 1.57 / 1.60 → 1.57 / 1.62 / 1.56 | 1.60 → 1.57 | 0.98 | 3.22 → 3.93 | 24000000 → 24000000 | MET after the D5 REVERT (was **MISSED**) |

  Every `sha256` is identical HEAD / tree on every line in all three sets.
  The tree's `views_after` is 0 on every line except `preview_after`
  (24000000, the masked-cache plane's view, as at HEAD). The two earlier
  sets are recorded only. The FIRST cut (runs everywhere, the D2 cursor;
  load `14.40 11.91 8.24` … `10.54 11.22 8.17`): `edit_u16` 0.95 ×,
  `edit_u16_p3` 1.02 ×, `edit_u8_master` 0.99 ×, `edit_byte_doc_u16`
  1.00 ×, `preview_before_p3` 473.78 → 507.63 ms, **1.071 × MISSED**,
  `preview_after` 1.60 → 1.97 ms, **1.229 × MISSED**; `views_held` 0 on
  every tree line. After the FIX PASS (D6, load `28.21 12.29 9.30` …
  `23.92 13.44 9.88`): `preview_before_p3` 424.79 → 436.56 ms, 1.028 × MET;
  `preview_after` 1.54 → 1.85 ms, **1.197 × still MISSED**, so it reverted
  (D5); the edit lines 0.96–0.99 ×. The diagnostic probes (a scratch
  example timing 7–21 previews per process, not committed) are summarized
  in D5 / D6.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-2/…`, run logs in
  `logs/`, `PYTHONDONTWRITEBYTECODE=1`, the app BINARY path):
  `verify_phase12e2b_placement.py` (`12e2b`, load `25.47 22.53 25.09` →
  `23.70 22.18 24.80`): ALL GATES PASS (13 PASS).
  `verify_phase12h_conversion.py` (`12h`, → `27.33 23.01 25.06`): 16 PASS,
  0 FAIL. `verify_phase12g_clipboard.py` (`12g`, → `11.79 18.47 22.67`): 13
  PASS, 0 FAIL. `verify_phase9h_ui.py --require-current --report …/9h.json`
  (`10.16 17.78 22.35` → `6.72 11.70 18.43`): 56 groups + pass 2: 1, every
  case PASSED, `current-layer-commands-ui` included. `verify_phase9b.py
  --fixtures …/9b-fixtures --report …/9b.json --require-current` (→ `10.16
  17.78 22.35`) FAILS in `filter-stack-probe/mcp.py`'s `destructive_twins`
  at depth 16. The probe expects `apply_filter` `unsharp_mask` to REFUSE in
  a 16-bit document, but the app applies it (`{"filter":"unsharp_mask",
  "storage":"rgba16_unorm","ok":true,…}`). This is a STALE probe
  expectation: since phase 9F (`2e7717d`, 2026-09-12) the app admits
  `unsharp_mask` / `high_pass` as selected spatial filters at 16 bits
  (`SelectedAdjustment.swift`). That path is `selected_filter` /
  `ops_photographic`, which this run does not touch. It was NOT re-run
  against a HEAD app, and it is recorded as an open item. No owned drive:
  no app-visible behaviour changes.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-3a)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~5–44, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a scratch copy (`git archive a2541cf core scripts`, with the new suite and
the example copied in). Nothing in the working tree was stashed or checked
out.

- Baseline at `a2541cf`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 15
  in 11 files; the seeded-site `rg` of the 13B-3d-3a contract = 22 lines.
- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_whole_readers --test
  selected_adjustments --test precision_scalar_selection --test
  filter_tests --test precision_channel_math --test stats_tests --test
  precision_paint --test precision_stats --test mask_outside_coverage
  --test precision_scalar_statistics --test precision_cutout --test
  refinement_cutout --test refinement_execution --test
  persisted_warp_native --test persisted_warp_numeric --test
  persisted_warp_safety --test precision_layer_producers --test
  precision_clipboard --test precision_constructor_seeds --test
  live_filter_tile_reads -- --test-threads=1 --nocapture` (load not
  recorded, ~20–30 around it): 20 suites, 154 passed, 0 failed.
- HEAD comparison. At `a2541cf` exactly `no_view_whole_reads` fails, by
  design (`selected/pixelate7: no view held`, the first case; HEAD binds
  the layer's view); `record_whole_reads`,
  `plane_read_matches_the_samples`, `pixelate_matches_the_block_means` and
  `cutout_layer_is_the_foreground` pass there. All 60 `whole` record lines
  are identical between HEAD and the tree (answers, checkpoint counts and
  report-log hashes), checked on the first cut, after the D7 fix pass and
  after the WARP encode's pass. All 54 `precision_layer_producers`
  `producer` record lines (the `warp/u16` lines included) are identical to
  HEAD's; its `no_view_layer_producers` now expects 0 for the warp and
  passes.
- `make test` twice on the final tree: 299 binaries, 2915 passed, 0 failed,
  27 ignored each (run 1 ended at load `13.98 26.30 24.21`, its start not
  recorded; run 2 `13.98 26.30 24.21` → `9.86 17.50 20.80`). No known
  flake recurred.
- `make cancel-bench` once, serial (load `9.23 17.25 20.69` → `7.12 15.60
  19.90`): 14 suites, 21 passed, 0 failed (the
  `regional_group_materialization_cancel` load residual did not recur).
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **9 = 2 definitions
  + 7 call lines** in 5 files (was 15 in 11). Seeded-site count: **22 → 20**
  lines.
- `make typecheck` — 0 errors (load `6.71 15.37 19.80`); `make app` — BUILD
  SUCCEEDED (load `4.91 10.80 17.09` → `36.78 18.52 19.64`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=whole_readers`, HEAD and
  tree alternated, 3 samples each. The bounds were FIXED before measuring
  (spec Constraints): `views_held` 0, binding; `sha256` equal; `ms_warm`
  median ≤ 1.05 × HEAD; `ms_first` recorded with no bound. Load before each
  run of the FINAL set: HEAD `24.92 21.93 18.97` / `43.74 26.60 20.74` /
  `33.43 25.54 20.52`, tree `25.68 22.20 19.10` / `36.75 25.90 20.59` /
  `28.32 24.81 20.35`.

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `selected_pixelate` | 6000×4000 | 532.1 / 538.5 / 531.2 → 497.8 / 514.4 / 488.0 | 532.1 → 497.8 | 0.94 | 610.8 → 535.8 | 192000000 → 0 | MET |
  | `selected_auto_color` | 6000×4000 | 211.2 / 204.1 / 203.0 → 203.9 / 205.5 / 212.3 | 204.1 → 205.5 | 1.007 | 230.4 → 209.9 | 192000000 → 0 | MET after the fix pass (was **MISSED**, 1.086) |
  | `read_plane_green` | 6000×4000 | 49.7 / 49.2 / 49.2 → 34.1 / 34.3 / 34.5 | 49.2 → 34.3 | 0.70 | 65.8 → 35.4 | 192000000 → 0 | MET |
  | `refinement_cutout` | 6000×4000 | 30.7 / 30.7 / 30.5 → 16.4 / 16.4 / 16.3 | 30.7 → 16.4 | 0.53 | 47.5 → 18.1 | 192000000 → 0 | MET after the fix pass (was **MISSED**, 1.611) |
  | `scalar_pixelate` | 6000×4000 | 588.3 / 592.5 / 591.4 → 584.5 / 576.0 / 561.3 | 591.4 → 576.0 | 0.97 | 652.4 → 639.2 | 0 → 0 | MET |
  | `scalar_auto_tone` | 6000×4000 | 132.5 / 133.8 / 140.5 → 133.3 / 130.7 / 133.9 | 133.8 → 133.3 | 0.996 | 132.0 → 135.9 | 0 → 0 | MET |
  | `warp_edit` | 3000×2000 | 690.0 / 684.1 / 682.5 → 712.8 / 693.7 / 687.8 | 684.1 → 693.7 | 1.014 | 691.5 → 708.8 | 48000000 → 0 | MET (the transient kept, D4) |
  | `warp_save` | 3000×2000 | 87.8 / 92.7 / 76.6 → 91.2 / 79.7 / 66.2 | 87.8 → 79.7 | 0.91 | 58.0 → 58.2 | 48000000 → 0 | MET (after its pass; 1.027 then 1.060) |

  Every `sha256` is identical HEAD / tree on every line in all three sets,
  and the tree's `views_held` and `views_after` are 0 on every line. The
  two earlier sets are recorded only. The FIRST cut (load `15.32 22.63
  18.40` … `8.25 19.06 17.36`): `selected_pixelate` 0.95 ×,
  `selected_auto_color` 204.9 → 222.5 ms **1.086 × MISSED**,
  `read_plane_green` 0.71 ×, `refinement_cutout` 30.5 → 49.2 ms **1.611 ×
  MISSED**, `scalar_pixelate` 0.94 ×, `scalar_auto_tone` 0.99 ×,
  `warp_edit` 1.006 ×, `warp_save` 1.027 ×. After the D7 fix pass (load
  `34.59 22.14 18.56` … `26.60 22.02 18.77`): `selected_auto_color` 1.006
  ×, `refinement_cutout` 0.552 ×, `warp_save` 80.1 → 84.9 ms 1.060 ×
  (was 1.027 ×; its own pass, D7), the others 0.70–1.004 ×. The final set
  above follows the WARP encode's pass.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-3a/…`, run logs
  in `logs/`, `drives-log.txt`, `PYTHONDONTWRITEBYTECODE=1`, the app BINARY
  path): `verify_phase12e2b_placement.py` (`12e2b`, load `32.96 18.86
  19.74` → `35.55 21.12 20.51`): ALL GATES PASS (13 PASS).
  `verify_phase12h_conversion.py`
  (`12h`, → `32.48 20.94 20.45`): 16 PASS, 0 FAIL.
  `verify_phase12g_clipboard.py` (`12g`, → `5.87 13.54 17.39`): 13 PASS, 0
  FAIL. `verify_phase9b.py --fixtures …/9b-fixtures --report …/9b.json
  --require-current` (→ `5.43 13.20 17.22`): PASSED (`"passed": true`; 30
  groups at depth 8, 26 at depth 16, among them the renamed
  `destructive_twins_apply_in_16_bit_as_selected_spatial_filters` and
  `cleanup_twins_apply_in_16_bit_as_selected_spatial_filters`) — the stale
  depth-16 expectations repaired in `filter-stack-probe/mcp.py`.
  `verify_phase12f3_warp.py --app … --receipts …/12f3` (→ `5.43 12.46
  16.81`): ALL GATES PASS 11. `verify_phase9h_ui.py --require-current
  --report …/9h.json` (→ `5.49 8.67 14.02`): 56 groups + pass 2: 1, every
  case PASSED, `current-layer-commands-ui` included (screen unlocked).
  `verify_phase9f.py --app … --report …/9f.json --require-current` (→ `5.55
  12.60 16.89`) FAILS at depth 16 before its persistence gate: it expects
  `apply_filter` `invert` with `target: "red"` to REFUSE in a 16-bit
  document, and the app applies it (`{"ok":true,"storage":"u16",
  "changed":true,"target":"red","filter":"invert","layer":0}`). This is a
  STALE probe expectation of the 9b kind: phase 9G (`f457cdb`, after 9F's
  `2e7717d`) added native channel editing, whose
  `AgentServer+PrecisionScalarEditing.swift` admits `apply_filter` with a
  channel `target` at 16 bits. The admission is Swift; this run changed no
  Swift and no admission (the scalar filter's gray image is only built
  unseeded). Not re-run against a HEAD app; recorded as an open item. No
  owned drive: no app-visible behaviour changes.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-3b)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~5–104, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a scratch copy (`git archive 58f8304 core`, with the new suite and the
example copied in). Nothing in the working tree was stashed or checked out.

- Baseline at `58f8304`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 9 in
  5 files; the seeded-site `rg` of the 13B-3d-3a contract = 20 lines.
- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test raw_develop_reads --test raw_local_numeric
  --test raw_local_native --test raw_source_geometry_numeric --test
  raw_source_geometry_native --test raw_source_geometry_safety --test
  raw_vignette_geometry --test develop_inspection --test
  precision_whole_readers --test selected_adjustments --test
  precision_cutout --test refinement_cutout --test doc_tests --
  --test-threads=1 --nocapture`: 13 suites, 103 passed, 0 failed.
- HEAD comparison. At `58f8304` exactly `no_view_raw_develop_reads` fails,
  by design (`develop/vignette: views held`, 4992000 against 2496000: HEAD
  also holds the base's view); `record_raw_develop_reads`,
  `preview_matches_the_cell_rule` and `exif_geometry_is_the_permutation`
  pass there. All 76 `raw` record lines (18 cases × 4 fixtures and the four
  `refuse/*` lines at 48×32, each asserting its sentence) are identical
  between HEAD and the tree — answers, checkpoint counts and report-log
  hashes — checked on the first cut (the preview on tile reads included),
  after the fix pass and after the revert.
- `make test` twice on the final tree: 300 binaries, 2919 passed, 0 failed,
  27 ignored each (run 1 load `9.27 13.30 12.72` → `26.24 39.97 29.37`, run
  2 → `12.52 26.93 26.07`). No known flake recurred.
- `make cancel-bench` once, serial (load `11.13 25.71 25.66` → `7.30 22.90
  24.63`): 14 suites, 21 passed, 0 failed.
- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **7 = 2 definitions
  + 5 call lines** in 4 files (was 9 in 5): the lend
  `precision_image.rs:307/323`, `live_filter_preview.rs:164`,
  `develop_inspect.rs:136`, `raw_source_geometry.rs:392`. Seeded-site
  count: **20 → 20** lines.
- `make typecheck` — 0 errors (load `6.72 22.52 24.49`); `make app` — BUILD
  SUCCEEDED (load `18.73 17.94 22.02` → `34.41 23.41 23.85`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=raw_develop`, HEAD and tree
  binaries alternated, 3 samples each (the three preview lines' samples are
  each the median of 9 warm calls). The bounds were FIXED before measuring
  (spec Constraints): `sha256` equal; tree `views_held` exactly the kept
  views; `ms_warm` median ≤ 1.05 × HEAD; `ms_first` recorded with no bound.
  Load before each run of the FINAL set (after the revert): HEAD `26.93
  17.08 13.51` / `16.86 16.15 13.39` / `12.27 14.93 13.08`, tree `23.26
  17.17 13.65` / `13.09 15.32 13.16` / `9.75 14.12 12.84`.

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `develop_vignette` | 6000×4000 | 5326.35 / 5411.01 / 5399.18 → 5339.70 / 5331.65 / 5397.10 | 5399.18 → 5339.70 | 0.989 | 5507.14 → 5392.82 | 384000000 → 192000000 | MET |
  | `develop_local` | 6000×4000 | 267.06 / 285.16 / 266.36 → 218.38 / 225.03 / 220.55 | 267.06 → 220.55 | 0.826 | 318.01 → 246.45 | 576000000 → 192000000 | MET after the fix pass (was **MISSED**, 1.052) |
  | `geometry_exif6` | 6000×4000 | 148.31 / 141.65 / 142.50 → 144.96 / 138.91 / 139.99 | 142.50 → 139.99 | 0.982 | 159.22 → 143.13 | 384000000 → 384000000 | MET (HEAD's code) |
  | `geometry_lens_bilinear` | 6000×4000 | 2976.55 / 2941.65 / 2972.66 → 2969.46 / 2964.33 / 2993.74 | 2972.66 → 2969.46 | 0.999 | 3042.49 → 3026.79 | 384000000 → 384000000 | MET (HEAD's code) |
  | `preview_fit` | 6000×4000 | 8.84 / 8.79 / 8.93 → 8.73 / 9.03 / 8.71 | 8.84 → 8.73 | 0.987 | 22.82 → 22.41 | 192000000 → 192000000 | MET after the revert (tile reads **MISSED**, 1.375 / 1.338) |
  | `preview_fit_clipping` | 6000×4000 | 61.91 / 61.78 / 62.36 → 62.01 / 63.37 / 62.54 | 61.91 → 62.54 | 1.010 | 76.55 → 76.43 | 192000000 → 192000000 | MET after the revert (tile reads **MISSED**, 1.359 / 1.057) |
  | `preview_detail` | 6000×4000 | 9.49 / 9.69 / 9.62 → 9.60 / 9.79 / 9.77 | 9.62 → 9.77 | 1.015 | 23.24 → 24.96 | 192000000 → 192000000 | MET after the revert (tile reads **MISSED**, 1.249 / 1.051) |

  Every `sha256` is identical HEAD / tree on every line in all three sets.
  No line fell through to 3000×2000. The FIRST cut (load `4.85–8.56`):
  `develop_vignette` 0.989 ×, `develop_local` 1.052 × **MISSED**, the
  geometry lines 1.003 × / 1.001 ×, `preview_fit` 1.375 ×,
  `preview_fit_clipping` 1.359 ×, `preview_detail` 1.249 × **MISSED**, the
  preview lines holding 0 views. After the fix pass (D3 / D4; load
  `9.68–23.61`): `develop_local` 0.827 ×, `develop_vignette` 0.974 ×, the
  geometry lines 0.98 ×, the preview lines 1.338 × / 1.057 × / 1.051 ×
  still **MISSED**, so the preview reverted (D4) and the final set above
  was measured on the reverted tree.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-3b/…`, run logs
  in `logs/`, `RZ_BACKGROUND=1 PYTHONDONTWRITEBYTECODE=1
  ANTHROPIC_API_KEY=dummy`, the app BINARY path):
  `verify_phase12e2b_placement.py` (`12e2b`, → `25.46 23.29 23.78`): ALL
  GATES PASS (13 PASS). `verify_phase12h_conversion.py` (`12h`, → `24.54
  23.16 23.72`): 16 PASS, 0 FAIL. `verify_phase12g_clipboard.py` (`12g`, →
  `35.32 25.54 24.34`): 13 PASS, 0 FAIL. `verify_phase9f.py --app … --report
  …/9f.json --require-current` (`59.86 31.10 26.33` → `103.51 44.83
  31.49`): PASSED (`"passed": true`, `native_snapshot_recovery` true at
  both depths, `source_check` 0 changes over 1271 files) — at depth 16
  `red` and `alpha` applied (`{"ok":true,"target":…,"filter":"invert",
  "storage":"u16","changed":true}`) and undid to the raw layer, `mask`
  refused "The target layer has no mask." and `channel:missing` refused
  `No channel named "missing" (list_channels shows them)`; persistence and
  recovery ran for the first time since 9G. `verify_phase9h_ui.py
  --require-current --report …/9h.json` (→ `50.24 75.05 57.74`): 55 of 56
  groups PASS, `current-layer-commands-ui` FAIL ("The owned editor never
  became the real key window (active=false, …, keyWindow=nil)"); rerun
  alone (`RZ_PAINT_UI_CASES=current-layer-commands-ui`, `…/9h-alone.json`,
  → `15.92 47.91 50.26`): the same FAIL. The screen was UNLOCKED (no
  `CGSSessionScreenIsLocked` key in `IOConsoleUsers`). The probe passes its
  environment to the app (`verify_phase9h_ui.py:100`), so the drives'
  `RZ_BACKGROUND=1` reaches it and `BackgroundLaunch.isEnabled` keeps the
  app from activating; this run changed no Swift (open item below).
  `verify_phase8d.py` (stale; 12B named 13D, but no 13D entry owns it) and `verify_phase8g.py` (needs the
  CC0 Nikon D70s fixture, absent here) were not run.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-4a)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~4–50, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a scratch copy (`git archive 6b97c48 core`, outside the repo and the
receipts folder, with the new suite and the example copied in). Nothing in
the working tree was stashed or checked out.

- Baseline at `6b97c48`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 7 in
  4 files; the seeded-site `rg` of the 13B-3d-3a contract = 20 lines.
- `cd core && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test host_intake_unseeded --test
  raw_develop_reads --test precision_image_tiles --test raw_local_numeric
  --test raw_local_native --test raw_source_geometry_numeric --test
  raw_source_geometry_native --test raw_source_geometry_safety --test
  raw_vignette_geometry --test develop_inspection --test
  precision_whole_readers --test host_source_packing --test raw_source_ops
  --test precision_metadata_bridge --test export_host_formats --
  --test-threads=1 --nocapture`: 15 suites, 97 passed, 0 failed, 1 ignored
  (before amendment 1; `precision_image_tiles` 9 passed, 1 ignored).
- HEAD comparison. At `6b97c48` exactly `no_view_host_intake` fails, by
  design (`intake/from_rgba16: views held`, 2496000 against 0: HEAD's
  intake seeds the view cache); `record_host_intake`,
  `intake_samples_match_the_host_buffer`, `peak_host_intake` and all four
  `raw_develop_reads` tests pass there. All 128 record lines (52 `intake`
  lines — 13 cases × 4 fixtures — and the 76 `raw` lines of
  `raw_develop_reads`) are identical between HEAD and the tree — answers,
  checkpoint counts and report-log hashes.
- Peak (`peak_host_intake`, 600×520, `--test-threads=1 --nocapture`; bytes
  above the baseline). The gate was FIXED before measuring (spec
  Constraints): tree `peak` / `held` ≤ HEAD on every line except
  `lend/seeded_u16`, whose bound is tree ≤ HEAD + `w·h·8` + 64 on both.

  | case | `peak` HEAD → tree | `held` HEAD → tree | gate |
  | --- | --- | --- | --- |
  | `intake/from_rgba16` | 5003475 → 5001729 | 5003475 → 2507211 | MET |
  | `intake/host_rgba16_le` | 5003475 → 5001739 | 5003475 → 2507211 | MET |
  | `pipeline/intake_develop` | 12493875 → 9997611 | 9996837 → 5004533 | MET |
  | `pipeline/intake_exif6` | 9999149 → 9999109 | 9996837 → 7500797 | MET |
  | `pipeline/intake_lens_bilinear` | 22478008 → 22477744 | 9996837 → 7500797 | MET |
  | `pipeline/intake_develop_lens` | 27471370 → 24975066 | 14990199 → 9998119 | MET |
  | `pipeline/intake_preview` | 5281923 → 5281923 | 5142683 → 5142683 | MET |
  | `lend/intake_u16` | 5003475 → 5003251 | 5003475 → 5003251 | MET |
  | `lend/seeded_u16` | 2496728 → 4992768 | 2496728 → 4992768 | MET (≤ 2496728 + 2496000 + 64 = 4992792; tree − HEAD = 2496040) |

- Residual `rg -c 'contiguous_rgba(16|32)\(' core/src`: **4 = 2 definitions
  + 2 call lines** in 3 files (was 7 in 4): `live_filter_preview.rs:164`,
  `develop_inspect.rs:136`. Seeded-site count: **20 → 17** lines.
- `make test` twice (before amendment 1): 301 binaries, 2923 passed, 0
  failed, 27 ignored each (run 1 load `49.80 103.59 61.14` → `10.01 51.88
  50.24`, run 2 `3.99 19.44 35.06` → `6.01 13.42 29.62`). No known flake
  recurred.
- `make cancel-bench` once, serial (load `5.85 13.26 29.47` → `5.00 12.13
  28.31`): 14 suites, 21 passed, 0 failed. The
  `regional_group_materialization_cancel` residual did not recur.
- `make typecheck` — 0 errors (load `4.92 11.99 28.16`); `make app` — BUILD
  SUCCEEDED (load `5.08 8.90 24.01` → `10.25 9.76 23.97`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=raw_intake` and
  `=raw_develop`, HEAD and tree binaries alternated, 3 samples each (the
  three preview lines' samples are each the median of 9 warm calls). The
  bounds were FIXED before measuring (spec Constraints): `sha256` equal;
  tree `views_held` exactly as declared; `ms_warm` median ≤ 1.05 × HEAD on
  `intake_rgba16`, `intake_host_pixels`, `pipeline_develop_lens` and the
  seven `typed/raw_develop` lines; `pipeline_exif6` and
  `pipeline_preview_fit` recorded with no bound; `ms_first` recorded with
  no bound. Load before each run: `raw_intake` HEAD `8.54 50.19 49.66` /
  `6.07 42.47 46.82` / `5.46 36.16 44.20`, tree `6.82 45.79 48.08` / `5.17
  38.77 45.33` / `4.89 33.10 42.81`; `raw_develop` HEAD `4.57 30.75 41.68` /
  `4.63 26.71 39.56` / `4.33 23.20 37.52`, tree `4.29 28.58 40.58` / `4.15
  24.82 38.50` / `4.61 22.03 36.77`.

  | line | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | `typed/raw_intake` `intake_rgba16` | 6000×4000 | 64.83 / 62.63 / 60.81 → 64.60 / 62.70 / 62.77 | 62.63 → 62.77 | 1.002 | 85.49 → 85.13 | 192000000 → 0 | MET |
  | `typed/raw_intake` `intake_host_pixels` | 6000×4000 | 160.14 / 161.82 / 161.69 → 157.60 / 163.28 / 164.55 | 161.69 → 163.28 | 1.010 | 166.46 → 169.88 | 192000000 → 0 | MET |
  | `typed/raw_intake` `pipeline_exif6` | 6000×4000 | 213.04 / 224.86 / 209.29 → 221.91 / 214.24 / 213.82 | 213.04 → 214.24 | 1.006 | 252.78 → 259.53 | 384000000 → 192000000 | recorded (no bound) |
  | `typed/raw_intake` `pipeline_develop_lens` | 6000×4000 | 8360.81 / 8350.92 / 8348.81 → 8449.51 / 8407.93 / 8332.99 | 8350.92 → 8407.93 | 1.007 | 8425.75 → 8454.50 | 576000000 → 192000000 | MET |
  | `typed/raw_intake` `pipeline_preview_fit` | 6000×4000 | 74.92 / 76.90 / 71.02 → 83.14 / 85.90 / 85.41 | 74.92 → 85.41 | 1.140 | 73.89 → 85.00 | 192000000 → 192000000 | recorded (no bound) |
  | `typed/raw_develop` `develop_vignette` | 6000×4000 | 5269.19 / 5303.58 / 5331.60 → 5323.50 / 5325.80 / 5307.48 | 5303.58 → 5323.50 | 1.004 | 5343.54 → 5347.12 | 192000000 → 0 | MET |
  | `typed/raw_develop` `develop_local` | 6000×4000 | 221.45 / 224.99 / 221.24 → 216.06 / 220.12 / 219.64 | 221.45 → 219.64 | 0.992 | 246.99 → 242.14 | 192000000 → 0 | MET |
  | `typed/raw_develop` `geometry_exif6` | 6000×4000 | 139.33 / 144.52 / 141.55 → 147.48 / 144.66 / 150.83 | 141.55 → 147.48 | 1.042 | 146.74 → 193.42 | 384000000 → 192000000 | MET |
  | `typed/raw_develop` `geometry_lens_bilinear` | 6000×4000 | 2967.60 / 2953.31 / 2953.59 → 3010.13 / 2986.98 / 2999.89 | 2953.59 → 2999.89 | 1.016 | 3028.38 → 3032.97 | 384000000 → 192000000 | MET |
  | `typed/raw_develop` `preview_fit` | 6000×4000 | 8.74 / 8.84 / 8.76 → 8.81 / 8.76 / 8.68 | 8.76 → 8.76 | 1.000 | 22.95 → 22.78 | 192000000 → 192000000 | MET |
  | `typed/raw_develop` `preview_fit_clipping` | 6000×4000 | 61.72 / 62.19 / 61.82 → 61.74 / 62.58 / 61.62 | 61.82 → 61.74 | 0.999 | 76.25 → 75.63 | 192000000 → 192000000 | MET |
  | `typed/raw_develop` `preview_detail` | 6000×4000 | 9.46 / 9.54 / 9.55 → 9.55 / 9.56 / 9.85 | 9.54 → 9.56 | 1.002 | 22.48 → 23.70 | 192000000 → 192000000 | MET |

  Every `sha256` is identical HEAD / tree on every line, and every tree
  `views_held` equals its declared value. No line fell through to
  3000×2000. No bounded line MISSED, so no fix pass ran.
- Deviations recorded by the implementer: the geometry output seed now sits
  at `raw_source_geometry.rs:470` (the `storage()` gate added 4 lines; the
  spec said `:466`), cited as `:470` in the new plan rows, the contract, D1
  and the test comments; after `cargo fmt` the develop output's `let image
  =` is on `raw_local_develop.rs:302` and the call wraps to `:303`; the 9h
  Move case was also run alone against the HEAD library (a diagnostic
  outside the spec, below).
- Drives against the built app, first run (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4a/…`,
  `RZ_BACKGROUND=1 PYTHONDONTWRITEBYTECODE=1 ANTHROPIC_API_KEY=dummy`, the
  app BINARY path): `verify_phase12e2b_placement.py` (load `13.69 10.63
  23.95`): ALL GATES PASS (13 PASS). `verify_phase12h_conversion.py` (`11.60
  10.51 23.29`): 16 PASS, 0 FAIL. `verify_phase12g_clipboard.py` (`10.95
  10.41 23.11`): 13 PASS, 0 FAIL. `verify_phase9f.py --require-current`
  (`4.80 7.31 18.37`): PASSED. `verify_phase9h_ui.py --require-current`
  (→ `3.93 5.24 14.00`): 54 groups, 2 failures — `current-layer-commands-ui`
  (the known key-window error, "The owned editor never became the real key
  window …") and `precision-move-handles-ui` ("Integer Move copied its
  native raster buffer"); the screen was LOCKED
  (`CGSSessionScreenIsLocked` true). `precision-move-handles-ui` alone
  FAILED the same way on the tree library and PASSED (`failures: []`) on
  the HEAD `6b97c48` library (`--library …/head6b97/…/librasterize_core.a`):
  `geometrygestures.swift` compared the lend pointers of two
  `layerSource(0)` handles, which HEAD's cache-keyed view made equal and
  D4's private per-handle copy makes always differ. The user chose a real
  storage identity (amendment 1, D6); D4 stays.
- Amendment 1 rerun on the final tree (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4a-a1/…`, run logs
  in `logs/`): `cargo fmt --check` and `make lint` clean; `cargo test
  --release --test precision_image_tiles --test host_intake_unseeded --test
  raw_develop_reads -- --test-threads=1` — 3 suites, 18 passed, 0 failed, 1
  ignored (`precision_image_tiles` 10 passed, one more:
  `the_storage_identity_names_the_shared_plane`); `rg -c` still 4 in 3
  files and the seeded sites still 17; `make test` twice: 301 binaries,
  2924 passed (one more than 2923), 0 failed, 27 ignored each (run 1 load
  `9.11 6.84 7.17` → `8.67 14.13 11.18`, run 2 → `6.31 10.51 10.21`);
  `make typecheck` 0 errors (load `5.57 10.21 10.11`); `make app` BUILD
  SUCCEEDED (`5.15 7.90 9.14` → `8.93 9.96 9.97`).
  `verify_phase9h_ui.py --require-current --report …/9h.json` (`7.63 9.65
  9.86` → `6.28 7.35 8.75`): PASSED, 56 groups, `failures: []` — including
  `precision-move-handles-ui` (`storage_equal` true) and
  `current-layer-commands-ui`; the screen was UNLOCKED (no
  `CGSSessionScreenIsLocked` key, before and after). `RZ_PAINT_UI_CASES=
  precision-move-handles-ui verify_phase9h_ui.py --report …/9h-move.json`
  (`5.74 7.19 8.66`): the case PASSED (`host-report.json`, copied to
  `9h-move-host-report.json`: `passed` true, `failures: []`,
  `storage_equal` true); the script's exit 1 is `REQUIRED_CASES`' "Required
  probe cases did not run", expected for a single-case run.
  `verify_phase12e2b_placement.py` (`4.08 6.34 8.19`): ALL GATES PASS (13
  PASS). `verify_phase12h_conversion.py` (`7.95 7.18 8.42`): 16 PASS.
  `verify_phase12g_clipboard.py` (`7.20 7.05 8.36`): 13 PASS.
  `verify_phase9f.py --require-current` (`6.55 6.85 7.88`): PASSED.
- The timing, peak, record-line and cancel-bench figures above predate
  amendment 1 and stand: the export it adds is called by nothing they run.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-4b)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~3–34, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a scratch copy (`git archive 277c88a core`, outside the repo and the
receipts folder, with the new suite and the example copied in). Nothing in
the working tree was stashed or checked out. The suite, example, `make
test` and `make cancel-bench` outputs are kept beside the drive logs
(`~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4b/logs/13b3e4b_*.txt`).

- Baseline at `277c88a`: the seeded-site `rg` of the 13B-3d-3a contract =
  17 lines; `rg -c 'contiguous_rgba(16|32)\(' core/src` = 4 in 3 files.
- `cd core && cargo fmt && cargo fmt --check && cd .. && make lint` — clean.
- `cargo test --release --test precision_decoders_unseeded --test
  precision_filled_outputs --test precision_ffi --test precision_image_tiles
  --test host_intake_unseeded --test precision_clipboard --test psd_depth16
  --test float_document --test persisted_warp_native --test
  retained_source_native --test precision_conversion_narrow --test
  develop_inspection -- --test-threads=1 --nocapture`: 12 suites, 110
  passed, 0 failed, 1 ignored. No other `core/tests/*.rs` assertion pinned
  a removed seed or a dense reopened uniform plane (`make test` below).
- HEAD comparison. At `277c88a` exactly `no_view_decoders` (`rz16/reopen:
  views held`, 7501776 against 0) and `reopened_uniform_planes_are_filled`
  (`600x520 .rz u16 add_layer_at`, `retained_bytes` 2505193 against the
  fresh plane's 735721) fail, by design; `decoded_samples_match_their_source`,
  `peak_decoders`, `record_decoders` and `resave_is_byte_identical` pass
  there. All 72 `decode` record lines (18 cases × 4 fixtures) are identical
  between HEAD and the tree — answers, checkpoint counts and report-log
  hashes.
- Views held (`no_view_decoders`, 600×520, after `evict()` with each answer
  held), HEAD → tree: `rz16/reopen` 7501776 → 0, `rz16/master` 7488000 → 0,
  `rzf/reopen` 9984000 → 0, `png16/open` / `tiff16/open` 2496000 → 0,
  `psd16/layered` 4992000 → 0, `psd16/composite` 2496000 → 0,
  `tiff32/open` 4992000 → 0, `rzcb16/decode` 2496000 → 0, `rzcbf/decode`
  4992000 → 0, `rgba8/payload` / `rzcb16/paste` 2496000 → 0,
  `narrow/u16_rgba8` 2502888 → 0, `narrow/f32_rgba8` 1248000 → 0,
  `render/u16` 7501776 → 0, `render/f32` 9984000 → 0,
  `pipeline/rz16_master_preview` 7488000 → 2496000 and
  `pipeline/png16_preview` 2496000 → 2496000 (each `w·h·8`, as declared).
- Peak (`peak_decoders`, 600×520, bytes above the baseline). The gate was
  FIXED before measuring: tree `peak` ≤ HEAD and tree `held` ≤ HEAD on
  every line.

  | case | `peak` HEAD → tree | `held` HEAD → tree | gate |
  | --- | --- | --- | --- |
  | `rz16/reopen` | 36216040 → 26944248 | 22523256 → 13251464 | MET |
  | `rz16/master` | 34084607 → 26596263 | 22495087 → 15006743 | MET |
  | `rzf/reopen` | 31540722 → 18017274 | 31540722 → 18017274 | MET |
  | `png16/open` | 7501047 → 7500895 | 5003475 → 2507211 | MET |
  | `tiff16/open` | 7502153 → 7502001 | 5003595 → 2507331 | MET |
  | `psd16/layered` | 14995904 → 10728515 | 9996709 → 3234733 | MET |
  | `psd16/composite` | 9997827 → 9996777 | 5003475 → 2507211 | MET |
  | `tiff32/open` | 16551261 → 14977203 | 11559663 → 6567399 | MET |
  | `rzcb16/decode` | 5001881 → 5001729 | 5001881 → 2505617 | MET |
  | `rzcbf/decode` | 9993881 → 9993729 | 9993881 → 5001617 | MET |
  | `rgba8/payload` | 5001881 → 5001729 | 5001881 → 2505617 | MET |
  | `rzcb16/paste` | 5007469 → 5001729 | 5005495 → 2509231 | MET |
  | `narrow/u16_rgba8` | 5688602 → 3754844 | 5684809 → 3181585 | MET |
  | `narrow/f32_rgba8` | 4996754 → 4996014 | 2503554 → 1255274 | MET |
  | `render/u16` | 22504786 → 13232994 | 17513770 → 8241978 | MET |
  | `render/f32` | 31538216 → 18014768 | 26546624 → 13023176 | MET |
  | `pipeline/rz16_master_preview` | 17493864 → 12501560 | 15077282 → 10085202 | MET |
  | `pipeline/png16_preview` | 7501047 → 7500895 | 5081483 → 5081483 | MET |

- Counts: the seeded-site `rg` **17 → 2** lines
  (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:470`); `rg -c
  'contiguous_rgba(16|32)\(' core/src` still **4** in 3 files.
- `make test` twice: 301 binaries, 2930 passed (2924 + the six new tests),
  0 failed, 27 ignored each (run 1 load `34.39 20.20 14.37` mid-run →
  `8.08 16.78 14.59`, run 2 `7.29 16.32 14.46` → `5.33 11.24 12.66`). No
  known flake recurred.
- `make cancel-bench` once, serial (load `5.14 11.10 12.61` → `4.84 10.27
  12.23`): 14 suites, 21 passed, 0 failed. The
  `regional_group_materialization_cancel` residual did not recur.
- `make typecheck` — 0 errors (load `4.45 10.10 12.16`); `make app` — BUILD
  SUCCEEDED (load `4.18 7.63 10.77` → `6.05 7.81 10.76`).
- `phase13b_tile_sharing` `RZ_TILE_SHARING_ONLY=decoders`, HEAD and tree
  binaries alternated, 3 samples each. The bounds were FIXED before
  measuring (D5): `sha256` equal; tree `views_held` exactly as declared (0,
  or `w·h·8` on the two preview lines); `ms_warm` median ≤ 1.05 × HEAD on
  all 11 lines; `ms_first` recorded with no bound. Load before each run:
  HEAD `26.31 19.35 13.15` / `16.70 17.79 12.87` / `10.97 16.19 12.50`,
  tree `21.25 18.66 13.05` / `13.73 17.03 12.71` / `9.46 15.51 12.35`.
  `reopen_rz16_render` and `open_psd16_render` fell through to 3000×2000
  (24 MP: the WARP's work limit; the layered PSD open's admission, "needs
  2399345460 bytes … working limit is 2147483648"), in both binaries.

  | line | depth | size | `ms_warm` HEAD → tree (3 samples) | median | ratio | `ms_first` median HEAD → tree | `views_held` HEAD → tree | bound |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | `reopen_rz16_render` | u16 | 3000×2000 | 146.95 / 153.57 / 153.78 → 152.12 / 150.35 / 153.66 | 153.57 → 152.12 | 0.991 | 178.67 → 171.19 | 144064016 → 0 | MET |
  | `reopen_rz16_master_preview` | u16 | 6000×4000 | 340.84 / 334.46 / 354.55 → 341.75 / 364.86 / 354.48 | 340.84 → 354.48 | 1.040 | 412.03 → 434.97 | 576000000 → 192000000 | MET |
  | `reopen_rzf_render` | f32 | 3000×2000 | 130.65 / 125.74 / 121.72 → 124.97 / 115.29 / 119.73 | 125.74 → 119.73 | 0.952 | 137.69 → 126.94 | 192000000 → 0 | MET |
  | `open_png16_preview` | u16 | 6000×4000 | 326.91 / 330.44 / 322.17 → 331.85 / 331.08 / 334.04 | 326.91 → 331.85 | 1.015 | 358.35 → 369.87 | 192000000 → 192000000 | MET |
  | `open_psd16_render` | u16 | 3000×2000 | 140.42 / 148.58 / 148.48 → 142.03 / 154.77 / 143.26 | 148.48 → 143.26 | 0.965 | 145.97 → 143.05 | 96000000 → 0 | MET |
  | `open_psd16_composite` | u16 | 6000×4000 | 496.75 / 506.94 / 503.69 → 506.95 / 508.49 / 491.04 | 503.69 → 506.95 | 1.006 | 494.75 → 499.44 | 192000000 → 0 | MET |
  | `open_tiff32_render` | f32 | 3000×2000 | 150.05 / 154.20 / 153.17 → 152.68 / 150.44 / 153.29 | 153.17 → 152.68 | 0.997 | 166.04 → 170.16 | 96000000 → 0 | MET |
  | `paste_rzcb16` | u16 | 6000×4000 | 519.35 / 528.21 / 524.97 → 521.18 / 523.60 / 513.59 | 524.97 → 521.18 | 0.993 | 553.02 → 544.29 | 192000000 → 0 | MET |
  | `paste_rzcbf` | f32 | 3000×2000 | 132.67 / 133.13 / 133.87 → 132.72 / 133.60 / 133.63 | 133.13 → 133.60 | 1.004 | 137.96 → 134.45 | 96000000 → 0 | MET |
  | `paste_rgba8` | u16 | 6000×4000 | 536.72 / 534.26 / 532.16 → 536.46 / 531.48 / 534.92 | 534.26 → 534.92 | 1.001 | 563.82 → 553.76 | 192000000 → 0 | MET |
  | `narrow_render` | u16 | 6000×4000 | 436.78 / 435.06 / 436.33 → 438.03 / 437.69 / 445.14 | 436.33 → 438.03 | 1.004 | 470.62 → 468.43 | 96000000 → 0 | MET |

  Every `sha256` is identical HEAD / tree on every line and sample, and
  every tree `views_held` equals its declared value. No line MISSED, so no
  fix pass ran and no site was reverted.
- `RZ_TILE_SHARING_ONLY=filled`, tree (load `7.02 14.24 12.01`) and HEAD
  (`6.54 14.02 11.95`), record only: `narrow_masked_adjustment` `views_held`
  96000000 → 0, `sha256` equal; the two `add_layer` lines unchanged.
- Deviations recorded by the implementer: after `cargo fmt` the float TIFF
  open's `let image =` stays on `float_io.rs:158` and the call wraps to
  `:159`; `precision_image_build.rs` is 616 lines (the spec's "≤ ~600");
  `no_view_decoders` prints a `views` line per case and asserts after the
  loop, so the HEAD run records every case's views (the spec's view lines);
  `reopened_uniform_planes_are_filled` builds each reference plane on the
  reopened document itself (the same profile `Arc`), and its dense cases run
  through both `.rz` and RZCB; `record_decoders` / `peak_decoders` also carry
  the render and pipeline cases.
- Drives against the built app (receipts
  `~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4b/…`, logs with
  the load line in `logs/`, `RZ_BACKGROUND=1 PYTHONDONTWRITEBYTECODE=1
  ANTHROPIC_API_KEY=dummy`, the app BINARY path):
  `verify_phase12e2b_placement.py` (load `5.58 7.65 10.67`): ALL GATES PASS
  (13 PASS). `verify_phase12h_conversion.py` (`11.52 8.78 10.87`): 16 PASS,
  0 FAIL. `verify_phase12g_clipboard.py` (`10.84 8.69 10.82`): 13 PASS, 0
  FAIL. `verify_phase12f3_warp.py` (`4.67 6.63 9.35`): ALL GATES PASS 11.
  `verify_phase12e3c_psd.py` (`4.50 6.52 9.28`): ALL GATES PASS (Photoshop
  answered; C1–C3 PASS, 20 MATCH lines, PD 8 / 16 the disclosed
  DIVERGENCE). `verify_phase11f_float.py` (`5.39 6.50 9.13`): ALL GATES
  PASS (11 PASS). `verify_phase9f.py --require-current` (`6.57 6.72 9.17`):
  PASSED. `verify_phase9h_ui.py --require-current --report …/9h.json`
  (→ `3.10 4.66 7.47`): 56 cases / 55 groups, one failure —
  `current-layer-commands-ui` ("The owned editor never became the real key
  window (active=false, visible=true, windowKey=false, keyWindow=nil)");
  the screen was LOCKED (`CGSSessionScreenIsLocked` true before and after).
  Rerun alone (`RZ_PAINT_UI_CASES=current-layer-commands-ui`,
  `…/9h-current-layer.json`, load `2.71 4.50 7.36`): the same FAIL, the
  screen still LOCKED before and after. This run changed no Swift.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-4c-1)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~3–34, recorded beside every timing. No other
`./build/…` Rasterize instance of ours was running. HEAD comparisons ran in
a FRESH scratch copy (`git archive 9a30000 core`, outside the repo and the
receipts folder, with the new suite copied in). Nothing in the working tree
was stashed. The suite, example, `make test`, `make cancel-bench`, build and
drive outputs are kept in
`~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4c-1/logs/13b3e4c1_*.txt`.

- Baseline at `9a30000`: `rg -c 'contiguous_rgba(16|32)\(' core/src` = 4
  in 3 files; the seeded-site `rg` = 2 lines (`live_filter_evaluate.rs:429`,
  `raw_source_geometry.rs:470`); `rg -c 'contiguous_rgba(16|32)\('
  core/tests core/examples` summed = 406 lines in 82 files (421 calls: 419
  in 81 test files, 2 in the example; plus 4 fn-path uses).
- Tree: `rg -c 'contiguous_rgba(16|32)\(' core/src core/tests
  core/examples` — no output (0); `rg -n 'preview_view_rgba16\(' core/src`
  — 3 lines (`precision_image.rs:288`, the definition;
  `live_filter_preview.rs:164`; `develop_inspect.rs:137`); the seeded-site
  `rg` — 2 lines (`live_filter_evaluate.rs:429`,
  `raw_source_geometry.rs:471`).
- `cd core && cargo fmt && cargo fmt --check && cd .. && make lint` — clean
  (`clippy::ok_expect` flagged four migrated `.ok().expect(..)` chains on
  the first pass; they are plain `.expect(..)`).
- `cargo test --release --test preview_view_exception --test
  precision_image_tiles --test precision_image_accessor --test
  precision_image_writers --test develop_inspection --test
  live_filter_tile_reads --test raw_develop_reads --test
  host_intake_unseeded --test precision_decoders_unseeded --test
  precision_stamp_flatten --test precision_resolution --test
  precision_canvas_size --test precision_structure_ops --test
  precision_insert -- --test-threads=1 --nocapture` (load `13.85 11.56
  8.62` → `17.82 12.60 9.06`): 14 suites, 96 passed, 0 failed, 2 ignored.
  The ignored `fallback_above_the_cache_bound_stays_contiguous`, run once
  (`--ignored --exact`, load `18.71 12.87 9.17`): passed.
- HEAD comparison: `cargo test --release --test live_filter_tile_reads
  --test raw_develop_reads --test host_intake_unseeded --test
  precision_decoders_unseeded --test preview_view_exception --
  --test-threads=1 --nocapture` at HEAD (load `19.78 13.19 9.31` → `34.32
  19.46 11.97`) and in the tree (`31.89 19.21 11.92` → `27.53 18.69
  11.82`): every suite passes in both, `preview_view_exception` included
  at HEAD. All 244 record lines (`live` 44, `raw` 76, `intake` 52, `decode`
  72) and all 18 `views` lines are identical between HEAD and the tree.
  Every `peak_host_intake` / `peak_decoders` line (27) has tree `peak` and
  `held` EQUAL to HEAD's (gate tree ≤ HEAD, fixed before measuring: MET on
  every line; e.g. `pipeline/intake_preview` peak 5281923 / held 5142683,
  `pipeline/png16_preview` 7500895 / 5081483 in both).
- `cargo run --release --example phase7_precision -- --accuracy-only`,
  HEAD vs tree: the 72-line outputs are identical (`diff` empty).
- `phase13b_tile_sharing`, 1 sample each, record only (no bound: the
  preview code is HEAD's). `RZ_TILE_SHARING_ONLY=live_filter` HEAD (load
  `18.91 17.38 11.55`) / tree (`12.68 15.93 11.28`), `ms_first` / `ms_warm`:
  `edit_u16` 90.17 / 89.09 → 91.65 / 88.96, `edit_u16_p3` 1015.54 / 986.88
  → 1021.61 / 1019.66, `edit_u8_master` 101.58 / 100.79 → 100.84 / 101.72,
  `edit_byte_doc_u16` 102.83 / 101.93 → 103.73 / 103.56,
  `preview_before_p3` 447.13 / 447.33 → 456.42 / 467.62, `preview_after`
  3.29 / 1.56 → 3.43 / 1.62 ms. `RZ_TILE_SHARING_ONLY=raw_develop` HEAD
  (`15.61 16.72 11.42`) / tree (`11.50 15.57 11.21`): `develop_vignette`
  5362.80 / 5352.39 → 5441.05 / 5212.41, `develop_local` 256.04 / 224.96 →
  242.31 / 220.22, `geometry_exif6` 196.26 / 158.07 → 198.21 / 161.33,
  `geometry_lens_bilinear` 3025.54 / 3008.78 → 3032.91 / 3004.23,
  `preview_fit` 23.38 / 9.14 → 22.76 / 8.72, `preview_fit_clipping` 74.92 /
  62.17 → 74.91 / 62.52, `preview_detail` 23.36 / 9.80 → 22.65 / 9.46 ms.
  Every `typed/live_filter` and `typed/raw_develop` line's `sha256` and
  `views_held` equals HEAD's.
- `make test` twice: 303 binaries, 2932 passed (2930 + the two new tests),
  0 failed, 27 ignored each (run 1 load `4.11 11.18 10.08` → `11.07 18.42
  14.85`, run 2 `10.04 17.83 14.70` → `4.76 11.77 12.73`). No known flake
  recurred.
- `make cancel-bench` once, serial (load `4.94 11.69 12.70` → `4.64 10.77
  12.31`): 14 suites, 21 passed, 0 failed.
- `make typecheck` — 0 errors (load `4.43 10.62 12.25`); `make app` — BUILD
  SUCCEEDED (load `4.54 8.80 11.27` → `13.31 10.48 11.81`).
- Drives against the built app (`RZ_BACKGROUND=1 PYTHONDONTWRITEBYTECODE=1
  ANTHROPIC_API_KEY=dummy`, the app BINARY path; `CGSSessionScreenIsLocked`
  absent — unlocked — before and after every drive):
  `verify_phase12e2b_placement.py` (load `10.75 10.09 11.63`): ALL GATES
  PASS (13 PASS). `verify_phase12h_conversion.py` (`23.94 13.42 12.76`): 16
  PASS, 0 FAIL. `verify_phase12g_clipboard.py` (`22.33 13.42 12.77`): 13
  PASS, 0 FAIL. `verify_phase9b.py --fixtures …/9b-fixtures --report
  …/9b.json --require-current` (`5.32 8.98 11.02`): passed.
  `verify_phase9h_ui.py --require-current --report …/9h.json` (`5.37 8.63
  10.82` → `3.21 5.89 9.10`): passed, 56 groups + pass 2: 1, no changed
  checkout inputs. No drive covers develop inspection (`verify_phase8d.py`
  is stale, owned by 13D; `verify_phase8g.py` lacks its fixture); the core
  suites above stand for it. This run changed no Swift.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-4c-2a)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~4–25, recorded beside every step. No other `./build/…`
Rasterize instance of ours was running. HEAD comparisons ran in a FRESH
scratch copy (`git archive 31981c3 core`, outside the repo and the receipts
folder, with the new suite copied in). Nothing in the working tree was
stashed. The suite, example, `make test`, `make cancel-bench`, build and
drive outputs are kept in
`~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4c-2a/logs/13b3e4c2a_*.txt`.
The gates below were fixed before measuring; none was widened.

- Baseline at `31981c3` (load `4.67 3.95 5.25`): `rg -n 'OutputView' core`
  71 lines in 25 files (21 source files, `core/CLAUDE.md`, 3 test files);
  the f32-view / alias `rg` 10 lines (`precision_clipboard.rs:262`,
  `precision_sampling.rs:56/69/78`, `core/CLAUDE.md:81`,
  `contiguous_views.rs:49`, `precision_image.rs:78`,
  `precision_image_build.rs:74/75/327`);
  `precision_image_build.rs` 616 lines; the seeded-site `rg` 2 lines
  (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:471`);
  `rg -n 'preview_view_rgba16\(' core/src` 3 lines.
- Tree: `rg -n 'OutputView' core` — no output; `rg -n
  'View::Rgba32|PlaneKey::Rgba32|Rgba32\(Weak|from_rgba32_extended_unseeded|fn output(16|32)' core`
  — no output; `wc -l core/src/precision_image_build.rs` — 594; the
  seeded-site `rg` (D3) — 2 lines (`live_filter_evaluate.rs:429`,
  `raw_source_geometry.rs:471`); `rg -n 'preview_view_rgba16\(' core/src`
  — 3 lines (`precision_image.rs:292`, `live_filter_preview.rs:164`,
  `develop_inspect.rs:135`; the two moved with the doc / import lines above
  them).
- `cd core && cargo fmt && cargo fmt --check && cd .. && make lint` —
  clean, first pass.
- `cargo test --release --test precision_f32_unseeded --test
  precision_image_tiles --test precision_image_writers --test
  precision_image_accessor --test precision_image_reads --test
  preview_view_exception -- --test-threads=1 --nocapture` (load `10.38
  10.12 8.00` → `14.88 11.28 8.52`): 6 suites, 39 passed, 0 failed, 3
  ignored. The ignored `fallback_above_the_cache_bound_stays_contiguous`,
  run once (`--ignored --exact`, load `15.05 11.38 8.57`): passed.
- HEAD comparison: `cargo test --release --no-fail-fast --test
  precision_f32_unseeded --test precision_plates --test
  precision_projection_unseeded --test precision_layer_producers --test
  precision_export_reads --test precision_projection_consumers --test
  raw_develop_reads --test precision_filled_outputs --test
  precision_decoders_unseeded --test precision_constructor_seeds --
  --test-threads=1 --nocapture` at HEAD (load `14.22 11.45 8.68` → `14.88
  11.89 8.92`) and in the tree (`14.88 11.89 8.92` → `12.50 11.54 8.86`):
  at HEAD the only failure is `no_view_f32_constructors` (by design:
  4992000 view bytes on each f32 case); the tree passes all 10 suites. All
  634 ` checkpoints=` lines (`f32` 32, `consumer` 33, `decode` 72,
  `export` 72, `filled` 92, `plates` 114, `producer` 54, `raw` 76, `seeds`
  63, `unseeded` 26) are identical (`diff` empty). Of the 22 `views` lines,
  19 are identical and the three f32 cases are HEAD 4992000 → tree 0
  (`ctor/from_rgba32`, `ctor/from_rgba32_extended`, `lend/seeded_f32`;
  control `ctor/from_rgba16` 2496000 in both). Of the 23 `peak` lines, 20
  are identical and three are lower (gates: tree `peak` ≤ HEAD and `held` ≤
  HEAD everywhere, and on these three `held` ≤ HEAD `held` − 4992000 —
  MET): `ctor/from_rgba32` and `ctor/from_rgba32_extended` peak 9984808 →
  9984576, held 9984808 → 4992544 (gate ≤ 4992808); `lend/seeded_f32` peak
  14976848 → 9984584, held 14976848 → 9984584 (gate ≤ 9984848);
  `rebuilt/f32` 5003520 / 11488 and `ctor/from_rgba16` 4992808 / 4992808 in
  both.
- `cargo run --release --example phase7_precision -- --accuracy-only`,
  HEAD vs tree (load `11.13 11.29 8.82` → `10.48 11.15 8.78`): the 72-line
  outputs are identical (`diff` empty).
- No production path changed, so no warm-timing bound applies and
  `phase13b_tile_sharing` was not re-run; `phase13a_regional_latency` was
  not measured (13A-latency).
- `make test` twice: 304 binaries, 2936 passed (2932 + the four new
  tests), 0 failed, 27 ignored each (run 1 load `10.04 11.05 8.76` →
  `20.10 21.53 15.30`, run 2 `20.10 21.53 15.30` → `15.82 15.87 13.94`).
  No known flake recurred.
- `make cancel-bench` once, serial (load `13.78 15.42 13.82` → `8.90 13.89
  13.34`): 14 suites, 21 passed, 0 failed.
- `make typecheck` — 0 errors (load `8.27 13.67 13.27` → `4.47 8.88
  11.27`); `make app` — BUILD SUCCEEDED (load `4.47 8.88 11.27` → `25.11
  13.59 12.85`).
- Drives against the built app (`RZ_BACKGROUND=1 PYTHONDONTWRITEBYTECODE=1
  ANTHROPIC_API_KEY=dummy`, the app BINARY path; `CGSSessionScreenIsLocked`
  absent — unlocked — before and after every drive):
  `verify_phase12e2b_placement.py` (load `20.48 13.27 12.75` → `19.90 14.20
  13.12`): ALL GATES PASS (13 PASS). `verify_phase12h_conversion.py`
  (`19.90 14.20 13.12` → `17.75 13.93 13.04`): 16 PASS, 0 FAIL.
  `verify_phase12g_clipboard.py` (`17.75 13.93 13.04` → `6.89 9.90
  11.42`): 13 PASS, 0 FAIL. `verify_phase9h_ui.py --require-current
  --report …/9h.json` (`6.65 9.80 11.38` → `7.20 7.35 9.71`): passed, 56
  groups + pass 2: 1, no changed checkout inputs. This run changed no
  Swift.
- `pgrep -f '^\./build/.*MacOS/Rasterize'` afterwards lists nothing. No
  `scripts/**/__pycache__`.

## Verification record (13B-3e-4c-2b)

All run on 2026-09-26 on this machine, which was shared and loaded by other
work: 1-minute load ~3–32, recorded beside every step. No Rasterize
instance was launched. Nothing in the working tree was stashed, checked
out or restored; the backed-out files were rewritten with `git show
d1f7601:<path> > <path>`. Receipts are in
`~/devel/rasterize-fixtures/phase13-receipts/13b/13b-3e-4c-2b/`
(`above-bound-tree.txt`, `above-bound-head.txt`, `tree-suites.txt` from the
tiling attempt; `amend1-*.txt` from the close).

- The probe (the tiling attempt's `precision_f32_above_bound` suite, now
  archived as `precision_f32_above_bound.rs` in the receipts folder),
  `cd core && cargo test --release --test precision_f32_above_bound --
  --ignored --test-threads=1 --nocapture`, once each, serially:
  - tree with the tiling code (load `25.39 22.19 15.67`, 317393 free
    pages): `above open/uniform_f32 8200x8193 peak=1343932821 held=11887
    ms=750 answer=err:precision operation needs 2151953720 bytes including
    retained inputs; working limit is 2147483648 bytes`;
  - HEAD `d1f7601` (a fresh `git archive d1f7601 core` scratch copy with
    the suite copied in; load `17.47 21.28 15.89`, 164968 free pages): the
    same line with `ms=606`;
  - the same refusal in both, so the suite panics at its first case
    (`the uniform TIFF opens`) and no later case ran. P = 1074921600
    (`8200·8193·16`); 2151953720 = 2·P + 2110520 — the float
    `finish_plan` admission, not the float-TIFF capacity admission.
- The tiling attempt's tree suites (`precision_f32_unseeded`,
  `precision_image_tiles`, `precision_image_writers`,
  `precision_image_accessor`, `precision_image_reads`,
  `preview_view_exception`, `float_exact_geometry`,
  `precision_filled_outputs`; load `27.64 22.50 15.70`) were green on the
  discarded code, and its fmt / lint were clean. History only: that code is
  backed out.
- `git diff --quiet d1f7601 -- core/src/precision_image_plane.rs
  core/src/precision_image_fill.rs core/src/precision_image_build.rs
  core/src/precision_sampling.rs core/tests/precision_image_reads.rs
  core/tests/precision_image_writers.rs` (load `22.21 30.56 21.12`):
  identical. `core/tests/precision_f32_above_bound.rs` absent and archived.
  `git diff --stat d1f7601`: exactly `core/CLAUDE.md`,
  `core/src/precision_image.rs`, `core/tests/precision_filled_outputs.rs`,
  `core/tests/precision_image_tiles.rs` and this file. The non-comment `-`
  / `+` lines under `core/src` and `core/tests` are exactly the renamed fn
  line (nit 3). `wc -l core/src/precision_image.rs` — 622.
- `cd core && cargo fmt && cargo fmt --check && cd .. && make lint` (load
  `3.43 5.37 8.45` → `7.34 6.05 8.48`): clean, first pass.
- `cargo test --release --test precision_image_tiles --test
  precision_filled_outputs --test precision_image_reads --test
  precision_image_writers -- --test-threads=1` (load `6.92 5.99 8.45` →
  `31.59 11.94 10.53`): 4 suites, 33 passed, 0 failed, 3 ignored (the
  three fallback tests); `the_report_counts_tiles_the_index_and_a_u16_seed_once`
  listed and passing.
- `make test` once (load `30.02 11.94 10.54` → `25.32 31.44 21.31`): 304
  binaries, 2936 passed, 0 failed, 27 ignored. No known flake recurred.
- Not run, because no compiled production code, Swift or FFI differs from
  `d1f7601` (the checks above prove it): the second `make test`, `make
  cancel-bench`, `make typecheck`, `make app`, the four drives, the
  HEAD-vs-tree record diffs, `phase7_precision`, `phase13b_tile_sharing`
  and the three ignored fallback tests (D4). `phase13a_regional_latency`
  was not measured (13A-latency).

## Open items

| Item | Owner |
| --- | --- |
| Asset-side tiles: `shared` rasters (placed / rendered sources) tiled before their first write without doubling their bytes — CLOSED in 13B-2c-2c (a dabbed placed source counts its asset once) | closed (13B-2c-2c) |
| Persistence / export / preservation / thumbnail / canvas-image / plate-copy / promotion reads (rzdc, the asset PNG writer, the WARP writer, PSD, `doc_preservation`, the `ffi_doc` thumbnail, `layer_canvas_image`, `inpaint_source.rs`'s plate copy, `precision_conversion_structure`) — CLOSED in 13B-2c-3a (0 views after a save / export / thumbnail / reopen; residual 25 → 14) | closed (13B-2c-3a) |
| The remaining 14-call `contiguous_image()` residual: the resample inputs (`doc_transform.rs:597/601`, `doc_geometry_set.rs:161`, `retained_source_render.rs:311`, `live_filter_evaluate.rs:346`), the temp-document reads (`doc_content_aware.rs:163`, `doc_retouch_output.rs:317`) and the seven unlisted sites (`doc_perspective.rs:329`, `doc_warp.rs:76`, `doc.rs:1512`, `doc.rs:1709`, `ffi_retouch.rs:195`, `live_filter_profile.rs:122`, `precision_scoped_source.rs:208`) — CLOSED in 13B-2c-3b (residual 0; 13B-2 closed) | closed (13B-2c-3b) |
| Reopen dedup: a reopened placed source decodes two equal planes — CLOSED in 13B-2c-3a (`materialize_byte` adopts the asset's plane on `same_bytes`; a 24 MP reopen `layer_pixels` 96000000, was 192000000) | closed (13B-2c-3a) |
| Partial re-sharing of an EDITED placed source on reopen (`rebuilt_from` over the asset): today a dab drops the layer's binding at save, so the reopened layer has no asset to share with (13B-2c-3a suite (i)) | 13C |
| Test hygiene: `projective_source_native.rs`'s `reject_bad_table` rejects any error containing `png`, and the refusal quotes a random tempdir path that can contain `png` (`.tmpngoBCM`, 13B-2c-3a `make test` run 1) — CLOSED in 13B-2c-3b (the guard reads the error with the path replaced by `<path>`) | closed (13B-2c-3b) |
| Warm-path cost of the transient input copy: a repeated edit on one input plane (second `warp_edit`, second live-filter edit) copies the plane per edit where it used to hit the view cache (`transforms/warm` #2, 3000×2000: warp 699.6 → 686.2 ms, live filter 272.3 → 277.1 ms, one sample each; the live-filter delta is of the order of one 24 MB copy). Since 13B-3e-2 `filtered` takes runs, so the byte `source8` could walk the asset's runs instead of its `to_contiguous_with_job` copy, at the cost of that copy's per-tile checkpoints (13B-3e-2 D4). Since 13B-3e-3a the TYPED warp pays the same per-call transient (`precision_warp.rs:85`, `transient_rgba16`): `typed/whole_readers` `warp_edit` 3000×2000 warm 684.1 → 693.7 ms (1.01 ×, final set, 3 samples each alternated at load ~25–44; 1.006 × and 1.004 × in the two earlier sets), within its ≤ 1.05 × bound. Since 13B-3e-4a the RAW source geometry pays one too (`raw_source_geometry`'s ONE `transient_rgba16` where HEAD re-read the input's seed): `typed/raw_develop` `geometry_exif6` warm 1.042 × (`ms_first` 146.74 → 193.42 ms) and `geometry_lens_bilinear` 1.016 ×, both MET; `typed/raw_intake` `pipeline_exif6` 1.006 ×, recorded (no bound); 3 samples each alternated at load ~4–9 | 13A-latency |
| Tiling transients: the `Vec` and the tiles alive together inside `new` / `unseeded` / `rebuilt` (unplanned scratch since 13B-2b) are not sized in `retained_source_render::work_plan`; size the term once for both depths — CLOSED in 13B-3b-1 (`+ n·4·depth`, both depths) | closed (13B-3b-1) |
| The 1 GiB contiguous-view bound (a projection working set above it re-materializes every layer) | 13B-6 |
| Typed accessor (`contiguous_rgba16/32`, `to_rgba16/32`, `ptr_eq`, `same_samples`; `as_rgba16/32` removed) — CLOSED in 13B-3a | closed (13B-3a) |
| Typed rasters onto tiles: `Pixels::U16/F32` → `TilePlane<[u16;4]>` / `<[f32;4]>` and the typed view cache — CLOSED in 13B-3b-1 | closed (13B-3b-1) |
| `rebuilt` / `with_rect` / `unseeded` / `reidentified` on `PrecisionImage` and their first callers (`render_high_depth`'s Shared arm, the whole-image typed writers); the constructors' seeds where nothing reads the plane whole (the 16-bit structure promotion, `layer_raster_persistence`'s promotion check) — CLOSED in 13B-3b-2 (`typed/dab` `precision_pixels` 1728000000 → 208777216; `typed/promote` 0 views, was 192000000 / 1; the promotion check asserts 0 views process-wide) | closed (13B-3b-2) |
| The typed dab path's cached views: up to ~1 GiB over the 13B-3a HEAD (960000000 bytes / 5 views after the 24 MP 8-dab history, unchanged by 13B-3b-2 because `rebuilt` seeds each state's buffer) — CLOSED for the PAINT dabs in 13B-3d-1 (`typed/dab` / `typed/dab_blend` hold 192000000 = the base's own seed, 1 view) and for the RETOUCH and transparency-locked dabs in 13B-3d-2 (`typed/retouch_dab` / `typed/locked_dab` hold 192000000, 1 view, were 960000000 / 768000000) | closed (13B-3d-1 paint, 13B-3d-2 retouch / locked); the cap itself 13B-6 |
| A zero-filled typed plane through `TilePlane::filled` (`add_layer_at`, the sourced shapes, the narrowing's masked-adjustment plane) instead of a zeroed `Vec` tiled — CLOSED in 13B-3d-3b (`filled_rgba16` / `filled_rgba32_extended`, `LayerRaster::filled`; a 24 MP u16 added layer's plane 192003072 → 1227776 bytes, 0 views) | closed (13B-3d-3b) |
| The resample outputs (`precision_sampling.rs:88/109/203`) Seeded for every caller: a per-caller `OutputView` (`precision_geometry.rs:151/154`, `retained_source_render.rs:373`, `develop_inspect.rs:89` Unseeded; `doc_export.rs:1316` Seeded) — CLOSED in 13B-3d-3b (`typed/plates` `geometry_affine_bilinear` / `geometry_resize_bicubic` / `geometry_rotate90_float` `views_held` 59564008 / 135000000 / 96000000 → 0) | closed (13B-3d-3b) |
| The `from_float_samples` writers' rebuild twin (`precision_adjustment.rs:296`, `float_document.rs:620`) — CLOSED in 13B-3d-2 (`rebuilt_float_samples` builds the destructive adjustment's output; `float_document.rs:620`'s f32 → u16 output shares no tile with its source — another storage — so it needs no twin and is untouched) | closed (13B-3d-2) |
| Reader-side `unseeded` (`precision_png`, `precision_codec`, `psd_import16`, `float_io`, `float_native`, `warp_native`, the clipboard codec) with the typed export reads; since 13B-3e-3b also the host intakes `ffi_precision_input.rs:76` (`rz_precision_doc_from_rgba16`) / `host_pixels.rs:236`, whose seed's whole reader is `raw_source_geometry.rs:392` (the app develops that base and passes it to the geometry, `RawDevelopmentRecipe.swift:428-438` / `RawLocalEvaluation.swift:141-143`): unseed them and move `:392` onto ONE transient together (13B-3e-3b D1) — the host-intake half CLOSED in 13B-3e-4a (both intakes Unseeded, `:392` onto ONE transient; `typed/raw_intake` intake lines 0 view bytes, were 192000000); the decoders CLOSED in 13B-3e-4b (`from_rgba16_decoded` / `from_rgba32_extended_decoded` at all ten reader-side sites, each checked against its whole readers — the 13B-3e-4b decisions' classification; `typed/decoders` 0 view bytes on nine lines, `w·h·8` on the two preview lines) | closed (13B-3e-4b; the host intakes 13B-3e-4a) |
| The promotion's first projection materializing a view per layer — measured 0 on the single-layer 24 MP promotion (`typed/promote` `contiguous_view_bytes_after_projection` 0, was 192000000 at HEAD, which was the promotion's own seed): the typed compositor binds no layer view; the projection OUTPUT's own seed (from the render constructors) is the remaining cost (13B-3c-1 pins it: `precision_image_reads` (e) — process-wide views after a projection and a window are exactly the two outputs' `w·h·bpp` seeds) — CLOSED in 13B-3c-2b (the output is `Unseeded`; (e) re-pinned at 0) | closed (13B-3c-2b) |
| The two FFI pointer exports (`rz_precision_image_pixels_rgba16` / `_rgba32`) lend the image's own `Arc`; once tiled the HANDLE must own the view it lends — CLOSED in 13B-3b-1 (`pinned_rgba16/32`) | closed (13B-3b-1) |
| The six raw-address test oracles (`precision_prototype`, `precision_canvas_rotation`, `precision_straighten`, `float_exact_geometry`, `placement_insert`, `retained_sources`) and `precision_color.rs:112/340`'s address-keyed dedup stop meaning "the image's allocation" once tiled; move them onto a tile-aware identity — CLOSED in 13B-3b-1 (`PrecisionImage::ptr_eq`; the dedup on `plane_key()`) | closed (13B-3b-1) |
| Typed reads off the views (compositor, regional typed tree, content-box, `precision_geometry`'s polled comparison over runs) — CLOSED in 13B-3c-1 (`row_segments` / `sample_at` / `runs_rgba16/32`; the typed content-box has no site, `scan_content_box` is byte-only; 0 view bytes over the document after a projection / regional window / float projection; residual 72 → 69) | closed (13B-3c-1) |
| Every consumer of a projection handle onto `runs` / `sample_at` (`quantize` / `quantize_scratch_bytes`, `tone_mapped_u16`, `precision_stats`, `float_stats`, `soft_proof`, `ffi_evaluation`'s compare, `precision_scoped_source`) — CLOSED in 13B-3c-2a (`rgba16_at`; every `typed/consumers` line 0 views after the cold call; outputs, checkpoints and reports equal to HEAD; residual 69 → 59) | closed (13B-3c-2a) |
| The projection OUTPUT's seed (render / regional / style-bake constructors unseeded), after classifying the other projection readers 13B-3c-2a's survey found (`ffi_retouch.rs:43`, `precision_clipboard.rs:231/233`, `precision_conversion_narrow.rs:758/768`, `precision_conversion_structure.rs:747`, `precision_color_policy.rs:347`) — CLOSED in 13B-3c-2b (`OutputView`; `typed/projection` / `typed/float_projection` / `typed/regional_window` 0 view bytes, were 384000000 / 192000000 / 16777216; residual 59 → 53) | closed (13B-3c-2b) |
| The retouch plates (`precision_retouch_source.rs:101`'s Seeded cut render and the Current arm at :64) and their readers (`precision_retouch.rs:388/427`, `precision_inpaint.rs:189`, `ffi_retouch.rs:43`'s bilinear taps); the selection source (`precision_selection.rs:125`, bound by the `\|i\|` closure at :132) — CLOSED in 13B-3c-2c-1 (plates and selection source `Unseeded`, their readers on rows or one transient; `typed/plates` `retouch_source_*` / `transform_retouch_source` `views_held` 0, were 240000000 / 192000000 / 384000000; `select_luminance_layer` 0, was 48000000) | closed (13B-3c-2c-1) |
| The Seeded LAYER renders (`precision_merge.rs:300/360/389/495`; the bake `precision_color_policy.rs` `bake_adjustment`, read by `convert_pixels` at :259) flipped once their next whole-layer writer stops binding the layer whole — CLOSED in 13B-3d-3a (all `Unseeded`; 24 MP `typed/producers` merge / stamp / flatten `views_held` 0, were 192000000; bake 0, was 384000000) | closed (13B-3d-3a) |
| The style bake (`precision_style_render.rs:919`, read whole by `psd_export16.rs:363`; D1), the Seeded export renders (`doc_export.rs:1270`, `doc_export_layers.rs:311`, `precision_io.rs:122`) and the host-projection export readers, which pay one materialization per unseeded handle (`doc_export.rs:1096/1317`, `float_io.rs:181`, `psd_export16.rs:499`; `export_png16` `ms_first` 1.31–1.33 × HEAD; `precision_sampling.rs:90` is a transient copy since 13B-3c-2c-1; `precision_codec.rs:742` walks runs since 13B-3d-3b) — CLOSED in 13B-3e-1 (the bake and the three export renders Unseeded; `convert16`, the sharpen, the trim / crop, `float_io.rs:181` and `psd_export16.rs:499` read runs, rows or one rect; every `typed/exports` line 0 views) | closed (13B-3e-1) |
| `precision_sampling.rs:65/90/183`'s resample-input binds (transient owned copies) and `tone_mapped_u16`'s output (`float_document.rs:620`) — CLOSED in 13B-3c-2c-1 (`transient_rgba16/32`; `tone_mapped_u16` takes `view`, classified per caller) | closed (13B-3c-2c-1) |
| The rest of 13B-3b-2's 29-site constructor seed classification: the 19 sites of the 13B-3c-2c-2 plan row — CLOSED in 13B-3c-2c-2 (every site classified with its readers and flip owner; the Copy / Cut payload and `convert` Unseeded; residual 45 → 41) | closed (13B-3c-2c-2) |
| The Seeded layer producers classified in 13B-3c-2c-2 whose every remaining whole reader is a whole-layer writer (e.g. `precision_paint.rs:330`): `artwork_render.rs:142`, `precision_warp.rs:117`, `precision_paint.rs:300`, `precision_insert.rs:132/141`, `precision_clipboard_convert.rs:117` — flipped once that writer stops binding the layer whole (`precision_paint.rs:330` no longer binds it since 13B-3d-1) — CLOSED in 13B-3d-3a for the artwork, the warp output, Rasterize and the converted paste (`from_rgba16_unseeded`); `precision_insert.rs:132/141` moved to 13B-3d-3b (`TilePlane::filled`) — CLOSED in 13B-3d-3b (`add_layer_at` over the filled constructors) | closed (13B-3d-3a; `add_layer_at` 13B-3d-3b) |
| The Seeded sites classified in 13B-3c-2c-2 with another whole reader: `live_filter_evaluate.rs:396/423` and `retained_source_render.rs:271` (`retained_source_render.rs:251`, `live_filter_evaluate.rs:409`, `live_filter_preview.rs:160`), `raw_local_develop.rs:264` / `raw_source_geometry.rs:466` (`raw_source_geometry.rs:392`, `raw_local_develop.rs:163/183`, `develop_inspect.rs:134`), `doc_export.rs:1117/1334` / `doc_export_layers.rs:135` (`doc_export.rs:1097/1323`), `precision_scalar_filter.rs:53/156` (`selected_filter.rs:263`, `selected_auto_levels.rs:52`) — the `doc_export` half CLOSED in 13B-3e-1 (`doc_export.rs:1117/1340`, `doc_export_layers.rs:135` Unseeded, their readers on runs); the live-filter / retained-source half CLOSED in 13B-3e-2 (`live_filter_evaluate.rs:396` and `retained_source_render.rs:272` Unseeded, `live_filter_evaluate.rs:409` and `retained_source_render.rs:251` on runs / rows; `live_filter_evaluate.rs:423` and its reader `live_filter_preview.rs:160` KEPT by the preview MISS, row below); the scalar-filter half CLOSED in 13B-3e-3a (`precision_scalar_filter.rs:53/156` Unseeded, their readers `selected_filter.rs:263` and `selected_auto_levels.rs:52` on rows / tile bands / runs; 13B-3e-3a D5); the raw half moved in 13B-3e-3b WITHOUT a flip: `raw_source_geometry.rs:466` stays Seeded because the develop preview MISS kept its reader `develop_inspect.rs:136` (D4, row below), and `raw_local_develop.rs:264` stays Seeded while its whole reader `raw_source_geometry.rs:392` remains (D1/D2); the develop reads `raw_local_develop.rs:163/183` walk runs; `raw_local_develop.rs:264` (now `:302`) CLOSED in 13B-3e-4a (Unseeded, its reader `raw_source_geometry.rs:392` on ONE transient) | 13A-latency (`raw_source_geometry.rs:471` with `develop_inspect.rs:137`; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception) |
| An in-memory Copy / Cut payload handle pasted directly (`clipboard_paste`'s no-convert arm, `from_clipboard`; tests only — the host pastes decoded payloads) installs an UNSEEDED layer since 13B-3c-2c-2; its next whole reader (e.g. `precision_clipboard.rs:492`) materializes a view then — CLOSED in 13B-3e-3a without code (its whole readers were the six moved onto tiles; `precision_whole_readers` `paste/read_plane` and `paste/from_clipboard_pixelate` hold 0 views; `precision_clipboard.rs:492` was a stale example, the float Cut's storage check, a rect writer since 13B-3d-1; 13B-3e-3a D6) | closed (13B-3e-3a) |
| Warm cost of the unseeded whole-layer Copy (`typed/clipboard` vs HEAD `71e4873`, 3 samples each: `copy_layer` 24 MP u16 80.6 → 93.2 ms, 1.16 × at load ~9–25, MISSED the ≤ 1.10 × bound; `copy_layer_float` 3000×2000 31.2 → 33.8 ms, 1.08 ×; a second set at load ~4–6, recorded only: 1.12 × / 1.12 ×). Cause (13B-3c-2c-2 review, traced, not re-timed): the in-call free of the adopted buffer when `tile_unseeded` replaces the contiguous plane with its tiles (`precision_image_build.rs:83`) — HEAD kept that buffer as the handle's view until the handle was freed; `encode` is not the cause | 13A-latency |
| Warm cost of the typed transient resample input: a repeated resample of one input plane copies it per call where HEAD hit the view cache (`typed/plates` `geometry_rotate90_float`, 3000×2000 f32, warm 52.4 → 63.4 ms, 1.21 ×, 3 samples each at load ~8–11; the u16 affine / resize / export lines ≤ 1.03 ×) | 13A-latency |
| `verify_phase9g.py` fails before its first gate: `scripts/live-filter-probe/native.py`'s `prefix` meets the 12E-1 outermost identity word 45/46, which no branch of `prefix` admitted — a stale probe reader, found in 13B-3c-2c-1 — CLOSED in 13B-3c-2c-2 (`_identity_body` peels 45/46 + `LIDS` and shifts the recorded positions; 9g passed, 7 + 20 groups) | closed (13B-3c-2c-2) |
| Warm cost of `scoped_layer_histogram` after 13B-3c-2a (vs HEAD `656a0aa`, review fix pass, 3 samples each at load ~14–17: 24 MP u16 266.1 → 306.5 ms warm (1.15 ×), 277.8 → 317.0 ms cold (1.14 ×)): a per-index `rgba16_at` through `mapped_xy` inside `scoped_statistics::evaluate`'s `|i|` callback instead of an indexed slice over a cached view; removing it needs a row-shaped callback. The review fix pass brought compare (0.54 ×) and the float histogram (1.00 ×) to parity or better; the u16 histogram is 1.07 × warm (0.92 × cold) | 13A-latency |
| Carried nit 1: the contiguous branches of `Plane::with_rect` / `read_rect` have no suite coverage (13B-3c-1 adds no rect read) — CLOSED in 13B-3d-1 (`precision_image_writers::j_contiguous_fallback_rect_write_and_read`, ignored, ~2.2 GB; passed) | closed (13B-3d-1) |
| Typed rect writers through `read_rect` + `with_rect`; the typed dab ≤ 4 tiles measurement — CLOSED for the PAINT writers in 13B-3d-1 (7D paint / erase / clone / heal, the five native tools, the float Cut's erase; `typed/dab` `unique_bytes` 208777216 with no whole-plane walk); the retouch in-place arm, `precision_scalar_edit`'s RGBA arm and the lock restore remain — CLOSED in 13B-3d-2 (the in-place retouch arm and the lock restore are rect writers; `precision_scalar_edit`'s RGBA arm reads runs + `rebuilt_rgba16`) | closed (13B-3d-1 paint, 13B-3d-2 the rest) |
| Plans still charge HEAD's whole-plane scratch after 13B-3d-1: `paint_plan`'s `output_pixels·8` and the native tools' `n·24 + output·8` (the rect walks allocate only the rect); tightening them changes `admit` / `job.reserve` answers | 13B-8 |
| A dab on a transparency-locked layer: `precision_lock::freezing_alpha` binds `src` / `dst` whole, and `dst` (the rect writer's output, no longer seeded) is now a cache miss — a whole materialization per locked dab (`precision_rect_writers`' `locked` case is exempt from the no-view assertion) — CLOSED in 13B-3d-2 (the restore reads and writes `unshared_rect`; `no_view_rect_writers` covers `locked`; `typed/locked_dab` 24 MP 80.57 → 11.12 ms per dab, 0 views beyond the base's seed) | closed (13B-3d-2) |
| `rebuilt_*`'s seed on the whole-image writers after 13B-3d-2 (`precision_scalar_edit`, `precision_mask_apply`, `selected_adjustment`, `precision_color_policy`, the destructive adjustment's `rebuilt_float_samples` — whose NO-CHANGE answer leaves the target's `w·h·8` view, as HEAD `a33e203` did (there its changed scan bound that same view): same footprint, only the holder moved) and the retouch NewLayer `from_rgba16` / grown-extent `rebuilt_rgba16` (Seeded) outputs — CLOSED in 13B-3d-3a (`rebuild_over` seeds nothing; `no_view_retouch_writers`' WHOLE and no-change arms assert 0; `typed/whole_writers` `apply_adjustment` `views_held` 0, was 192000000) | closed (13B-3d-3a) |
| The residual after 13B-3e-4a: 2 call lines, `rg -c` 4 in 3 files (`live_filter_preview.rs:164`, `develop_inspect.rs:136`); the lend and `:392` CLOSED in 13B-3e-4a — CLOSED in 13B-3e-4c-1 (both reads on `preview_view_rgba16`, the kept exception, and the accessor removed; residual 0, `rg -c` 0) | closed (13B-3e-4c-1) |
| 13B-3e-2 preview MISS: the 16-bit live-filter preview on tile reads (one `row_segments` per output row, a forward cursor) measured `preview_after` (2000×1500, divisor 2) warm 1.60 → 1.97 ms (1.23 ×, load ~11–14), and 1.54 → 1.85 ms (1.20 ×, load ~24–31) after the fix pass, against the ≤ 1.05 × bound. So `live_filter_preview.rs:160` reads `contiguous_rgba16()` and `live_filter_evaluate.rs:423` seeds again (13B-3e-2 D5). Residual: each typed live-filter edit holds its filtered plane's view (`w·h·8`, 48000000 at 6 MP), and an unfiltered converted preview materializes `color16`'s output once per call. Cause (probes): a sampled row spans 8 tile runs of 2 KiB where HEAD read one contiguous 16 KiB row, and neither a restructured cursor nor a burst row copy recovered it | 13A-latency (the tile row read; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception) |
| 13B-3e-3b preview MISS: the develop inspection preview on tile reads (a tile band walk, D4) measured `typed/raw_develop` (24 MP, 9 warm calls per sample, median of 3 alternated samples) `preview_fit` 8.80 → 12.10 ms (1.375 ×), `preview_fit_clipping` 62.49 → 84.95 ms (1.359 ×), `preview_detail` 9.57 → 11.95 ms (1.249 ×) at load ~5–9, and 1.338 × / 1.057 × / 1.051 × at load ~10–24 after the fix pass, against the ≤ 1.05 × bound. So `develop_inspect.rs:136` reads `contiguous_rgba16()` and `raw_source_geometry.rs:466` seeds again (D4). Residual: a preview binds its source's view (`w·h·8`, 192000000 at 24 MP; the geometry output's seed when the app previews a geometry result) | 13A-latency (the tile row read; since 13B-3e-4c-1 the read is `preview_view_rgba16`, the kept exception) |
| The canvas-proportional passes an in-place retouch dab still pays after 13B-3d-2 (no plane read, no view): the source plate (the Current arm's canvas-sized row copy), the source transform (a canvas-sized sampled plane), and the scans of the canvas-sized coverage (selection domain, admitted-bounds check, `any`, the kernel's bbox) — `typed/retouch_dab` 24 MP 599.83 ms per dab, 21889 checkpoints (HEAD 776.20 ms, 39447); the scans need a rect-shaped FFI input | 13B-9 |
| Warm cost of the whole-image writers' run walks after 13B-3d-2 (vs HEAD `a33e203`, 3 samples each alternated at load ~8.6–11.7): `typed/whole_writers` `apply_adjustment` 24 MP `ms_warm` 353.28 → 361.01 ms (1.02 ×) MISSED the ≤ 1.00 × bound after the review fix pass's tile-band changed scan (first cut 1.06 ×) — HEAD read a cached contiguous view (the base's own seed), the tree walks tiles; `typed/scalar_dab` (first cut 1.04 ×) is 0.85 × after the tile-band `read()`, MET; `typed/retouch_dab` 0.77 × missed its 0.75 × EXPECTATION (its ≤ 1.00 × bound MET) | 13A-latency |
| A GROWN native-tool answer (paint_blend / bucket_fill / gradient whose changed ink leaves the layer, incl. a layer off the canvas) still rebuilds whole through `rebuilt_rgba16` and leaves an `nw·nh·8` view (rows, no bind) — CLOSED in 13B-3d-3a (the rebuild stays whole — new dimensions — but seeds nothing; `no_view_rect_writers` asserts 0 for a GROWN answer) | closed (13B-3d-3a) |
| The first whole read of an installed layer after 13B-3d-3a: the typed native save of a freshly flattened 24 MP document (`typed/producer_consumers` `save_after_flatten`, `precision_codec.rs:742` via `precision_native.rs:508`) materializes the layer's view once, `ms_first` 108.66 → 179.53 ms (1.65 ×), warm 97.88 → 109.71 ms (1.12 ×), 3 samples each at load ~10–24 (recorded, no bound) — the SAVE half CLOSED in 13B-3d-3b (`encode` walks runs; `views_held` 192000000 → 0, `ms_first` 1.04 × `63421b3`); the other whole readers (`precision_warp.rs:85`, `warp_native.rs:217`, `selected_filter.rs:263`, `selected_auto_levels.rs:52`, `precision_plane_read.rs:199`; `float_native.rs:73` and `psd_export16.rs:363` walk runs since 13B-3e-1) pay the same one materialization. Since 13B-3e-2 so does the placed live-filter / retained-source layer when its render is `color16`'s output or the promoted byte master, both unseeded and placed by `render_high_depth`'s Shared arm (`reidentified`); a FILTERED placement is the seeded filtered plane (the preview MISS) and pays none while that view is cached — CLOSED in 13B-3e-3a (the five other whole readers read tiles, and the warp one transient per call: `typed/whole_readers` `views_held` 0 on every line, were 192000000 / 48000000; the placed live-filter / retained-source layer's whole readers are the same ones, so it pays none either; the only whole bind of such a layer left is the FFI lend, 13B-3e-4); the lend CLOSED in 13B-3e-4a (private copy, no view) | closed (13B-3e-3a; the lend 13B-3e-4a) |
| The filled plans' whole-plane charges: `StructureEdit::scratch_bytes`' AddLayer `w·h·bpp`, the promotion plan's `walk.shape` and the narrowing's `walk.output` still charge a dense plane where the filled plane allocates ≤ 4 tiles (13B-3d-3b D4) | 13B-8 |
| A reopened empty layer decodes a DENSE plane (the `.rz` / PSD readers build the added layer's zeros as a full plane; only a fresh `add_layer_at` is filled) — CLOSED in 13B-3e-4b (a bitwise-uniform decoded plane is ONE filled plane through `uniform_tiles`: the reopened `.rz` u16 / f32 `add_layer_at` layers, the PSD pixel-less record, a uniform `.rz` layer and a uniform RZCB payload retain exactly a fresh `add_layer_at` plane's bytes, `reopened_uniform_planes_are_filled`; admission still charges the dense plane, next row's 13B-8 item) | closed (13B-3e-4b) |
| The byte `adding_layer_at` zero plane (`layer_insert.rs:218`, a zeroed `RgbaImage` through `LayerRaster::new`) — `LayerRaster::filled` exists since 13B-3d-3b | 13B-5 |
| The export renders' `Seeded` (`doc_export.rs:1270`, `:1320`) now serves only the sharpen (`doc_export.rs:1329`) and `convert16` (`:1097`): `encode` walks runs since 13B-3d-3b — CLOSED in 13B-3e-1 (both Unseeded; the sharpen and `convert16` walk runs) | closed (13B-3e-1) |
| `OutputView::Seeded` has no caller since 13B-3e-1 (`#[allow(dead_code)]`); remove the variant with its arms (`precision_render.rs:789/805`, `float_document.rs:627`, `precision_sampling.rs:64/78`) — CLOSED in 13B-3e-4b (the variant, its five arms and the seeded render constructors `from_float_samples` / `from_rendered_float` removed) | closed (13B-3e-4b) |
| `OutputView` has one variant (`Unseeded`) since 13B-3e-4b; remove the type and its `view` parameters: 9 functions take one (`precision_render.rs:728`, `precision_document.rs:860/874`, `float_document.rs:611`, `precision_scoped_source.rs:110`, `precision_sampling.rs:61/74` — the private `output16` / `output32` — and `:88/211`, `transform_image` / `resize_image`), with 36 `OutputView` mentions across 19 source files (21 counting the two doc-only mentions in `precision_image*`) — CLOSED in 13B-3e-4c-2a (the type, its nine parameters and `output16` / `output32` deleted; `rg -n 'OutputView' core` 71 lines → none) | closed (13B-3e-4c-2a) |
| Warm cost of the export writers' run walks after 13B-3e-1 (`typed/exports` vs HEAD `2c04c47`, 3 samples each alternated at load ~11–34): `save_psd_flattened` 24 MP `ms_warm` 902.59 → 977.31 ms (1.08 ×) and `save_native_float` 3000×2000 13.70 → 17.48 ms (1.28 ×) MISSED the ≤ 1.05 × bound — HEAD's warm call reads the contiguous view its first call cached, the tree walks tile runs every call (its `ms_first` 834.07 / 15.99 beats HEAD's 845.23 / 30.12); the PSD channel extraction ~1.1 × HEAD's slice passes after D5, the float staging fill from 4 KiB runs unchanged by D6. Carried nit 1 (recorded in 13B-3e-2): the flattened PSD walks the projection twice — `channels16` and `composite_planes16` — and builds alpha twice. Merging the two walks is the visible saving and needs a `composite_planes16` signature change | 13A-latency |
| The narrowing's remaining seeds: `precision_conversion_narrow.rs:233` (the base raster, `LayerRaster::new` of the quantized plane), `:400` (source pixels) and `:127` (`unit_pixels`); `typed/filled` `narrow_masked_adjustment` holds exactly the base's 96000000 view bytes — CLOSED in 13B-3e-4b (all three `LayerRaster::unseeded`; `narrow_masked_adjustment` `views_held` 96000000 → 0, `typed/decoders` `narrow_render` 96000000 → 0, `precision_filled_outputs::no_view_filled_planes` asserts 0) | closed (13B-3e-4b) |
| Warm cost of the typed native save's run walk after 13B-3d-3b (`typed/producer_consumers` `save_after_flatten`, 24 MP, vs HEAD `e250ba6`, 3 samples each alternated): `ms_warm` 113.26 → 119.71 ms (1.06 ×) MISSED the ≤ 1.05 × bound at a falling load of 27–85 → ~5 — HEAD's warm save reads the view its first save cached, the tree walks tile runs on every save; a second set at a steady load of 4–9 (recorded only) measured 118.46 → 101.63 ms (0.86 ×) | 13A-latency |
| Typed `.rz` / PSD / export reads off the views; residual 72 → 0 (13B-3b-1 recount) — the export / PSD / float writers CLOSED in 13B-3e-1 (residual 15 call lines); the live filter and the retained source CLOSED in 13B-3e-2 except the preview read (residual 13 call lines, the preview MISS row); the installed-layer whole readers CLOSED in 13B-3e-3a (residual 7 call lines); the raw develop reads CLOSED in 13B-3e-3b (residual 5 call lines: the lend, the two kept preview reads, `raw_source_geometry.rs:392`); the lend and `raw_source_geometry.rs:392` CLOSED in 13B-3e-4a (residual 2); the two preview reads CLOSED in 13B-3e-4c-1 (onto `preview_view_rgba16`, the kept exception; residual 0) | closed (13B-3e-4c-1) |
| Typed planes above `CONTIGUOUS_CACHE_BYTES` (f32 above 67,108,864 px) stay contiguous (13B-3b-1 decision (c)); tile them once no read needs a whole view (13B-3c-1's reads serve the fallback in place: `precision_image_reads` (g); 13B-3c-2b's unseeded renders still tile, so such a plane stays contiguous and unseeded; 13B-3c-2c-1's transient readers BORROW such a plane, no copy; 13B-3c-2c-2's share needed no tiling change — `tiles_of` refuses above the bound for both tilings, the unseeded f32 payload `precision_clipboard.rs:262` keeps the fallback and its readers' runs serve it in place, as do `precision_insert.rs:132` and `convert`'s f32 target; f32 whole readers remain: `float_native.rs:73`, `float_io.rs:181`; 13B-3d-2 removed `precision_lock.rs:289/291`, whose restore now reads `read_rect_rgba32`, served in place by the fallback; since 13B-3e-1 no f32 whole reader remains in `core/src` — `float_native.rs:73` and `float_io.rs:181` walk runs, served in place too — but the FFI lend `precision_image.rs:322` still binds one; since 13B-3e-4a the lend binds no view (a contiguous plane lends its own buffer); since 13B-3e-4c-1 no f32 whole-view reader exists (`contiguous_rgba32` removed); since 13B-3e-4c-2a no f32 constructor seeds and the cache has no f32 view type, so the fallback's only open question is the cost of tiling it: its tiling transient and the `transient_rgba32` / `Plane::lent` / `with_rect` copies that today borrow it, 13B-3e-4c-2b D1) — CLOSED as a recorded residual in 13B-3e-4c-2b (the 8200×8193 all-0.0 float TIFF open is refused at HEAD `d1f7601` and with the tiling code alike: "needs 2151953720 bytes … working limit is 2147483648", 2·P + 2110520 for P = 1074921600, so no admitted document path retains such a plane; tiling code backed out; the question → 13B-8, next row) | closed (13B-3e-4c-2b, residual) |
| f32 planes above `CONTIGUOUS_CACHE_BYTES` stay contiguous (HEAD's rule). They are reached only by direct `from_rgba32*` and, code-derived and unmeasured, by a transient f32 render over an enlarged canvas (13B-3e-4c-2b D2's example). Tile them only if 13B-8's tighter plans ever admit one into a document. The tiling transient and the `transient_rgba32` / `Plane::lent` / `with_rect` copies that today borrow the plane then need a charge or a tile walk | 13B-8 |
| The typed geometry plan charges no input transient. `work` (`precision_geometry_plan.rs:168-197`) gives affine / perspective `temporary` 0, and `resize_cost` (`precision_sampling.rs:238-265`) charges only its separable intermediate. Meanwhile `transform_image` / `resize_image` bind ONE whole `transient_rgba16/32` copy of a tiled input (`precision_sampling.rs:71/93/187`): an uncharged `w·h·bpp` per call since 13B-3c-2c-1. It replaced an uncharged view materialization, and no f32 seed has stood in for it since 13B-3e-4c-2a. Charge it or state why not — the u16 half CLOSED (`precision_sampling::source_copy_bytes`, 8 B/px, in the plan's per-plane transient max; `precision_geometry_scratch.rs`), and the u16 destination adoption copy CLOSED (`precision_sampling::adoption_copy_bytes`, 8 B/px + 64 B/tile, charged beside the source copy as copy + max(temporary, adoption); `precision_geometry_scratch.rs` measures a 2× upscale with no allowance), and the f32 half CLOSED (`precision_sampling::float_source_copy_bytes`, 16 B/px; `float_adoption_copy_bytes`, 16 B/px + 64 B/tile) | 13B-8 |
| `verify_phase9b.py` fails at depth 16 in `filter-stack-probe/mcp.py`'s `destructive_twins`: it expects `apply_filter` `unsharp_mask` / `high_pass` to refuse in a 16-bit document, which the app has admitted as selected spatial filters since phase 9F (`2e7717d`); a stale probe expectation found in 13B-3e-2, not caused by it (not re-run against a HEAD app) — CLOSED in 13B-3e-3a (`destructive_twins` / `cleanup_destructive_twins` now assert each filter APPLIES at depth 16 and undoes cleanly, groups renamed `…_apply_in_16_bit_as_selected_spatial_filters`; 9b PASSED) | closed (13B-3e-3a) |
| `verify_phase9f.py` fails at depth 16 in `run_depth`: it expects `apply_filter` `invert` with a channel `target` (`red`, first of `red` / `alpha` / `mask` / `channel:missing`) to refuse in a 16-bit document, which the app has admitted since phase 9G's native channel editing (`f457cdb`, `AgentServer+PrecisionScalarEditing.swift`); a stale probe expectation found in 13B-3e-3a, not caused by it (no Swift or admission change; not re-run against a HEAD app). The gates after it (persistence, recovery) did not run — CLOSED in 13B-3e-3b (the depth-16 block asserts the 9G sets fixed from the source: `red` / `alpha` apply with reply `target` / `filter` / `storage: u16` / `changed` and undo restores the raw layer; `mask` "The target layer has no mask." and `channel:missing` `No channel named "missing"` refuse with their text; 9f PASSED including persistence and recovery; 13B-3e-3b D5) | closed (13B-3e-3b) |
| `verify_phase9h_ui.py` `current-layer-commands-ui` fails ("The owned editor never became the real key window") — reconciled in 13B-3e-4b with the lock-state evidence: every FAIL since 13B-3e-4a coincided with a LOCKED screen (13B-3e-4a first full run: `CGSSessionScreenIsLocked` true; 13B-3e-4b full run and the case alone: true before and after), and the case PASSED under `RZ_BACKGROUND=1` with the screen unlocked (13B-3e-4a amendment 1, 56/56, no `CGSSessionScreenIsLocked` key), so `RZ_BACKGROUND=1` alone does not fail it. The one unlocked FAIL on record is 13B-3e-3b's (full run and alone, lock read from `IOConsoleUsers`: no key); it has not recurred on an unlocked screen since. Re-scoped: a locked-screen limitation of the probe (it needs a real key window), the same as the closed 13B-2c-2b row; reopen only on an unlocked-screen FAIL | Phase 9H probe (`scripts/verify_phase9h_ui.py`) / `RZ_BACKGROUND` (58f8304), outside 13B — for the orchestrator to assign |
| `evict_caches` drops typed views too, but its `scope` string names only layer views and `removed_cache_bytes` counts only the supplied byte documents' views | 13B-6 |
| Typed op latency with the tiling copy and view materialization (`typed/dab` +29 %, `typed/projection` cold +30 % / warm +42 %, `typed/save` +27 % vs HEAD `09c7875`, one sample each under load). By cost: the DAB's whole-plane tiling → 13B-3b-2 `rebuilt` (after-figure: 3-sample median 839.7 ms vs 848.7 ms on the HEAD `634746c` library at the same load — the diff walk still visits every tile, so no wall-clock gain) then 13B-3d (rect writes, no whole-plane walk) — the dab part CLOSED in 13B-3d-1 (24 MP `typed/dab` `ms_median` 107.67 → 10.89 ms per dab, 0.10 × HEAD `97e5825`; `typed/dab_blend` 238.98 → 11.19 ms, 0.05 ×; 3 samples each at load ~98–154); the PROJECTION OUTPUT's tiling (the render constructors) and the cold-projection view rebuilds → 13B-3c (13B-3c-1: the 24 MP u16 projection cold 447–459 → 353–359 ms, warm 412–425 → 317–321 ms vs HEAD `740294b` at load ~6, 3 samples each; the output's tiling and seed → 13B-3c-2b); the SAVE's view rebuild → 13B-3e | 13A-latency (removed as 13B-3c / 3d / 3e take the copies away) |
| `precision_image.rs` is 772 lines, above the ~600 soft cap, after the flip (the spec kept the representation in one file) — CLOSED in 13B-3b-2 (439 lines; `precision_image_plane.rs` 238, `precision_image_build.rs` 485, both `#[path]` children) | closed (13B-3b-2) |
| `regional_group_materialization_cancel`'s "poller stalled past the stage" residual failed 3 of 3 serial runs in 13B-3a, including one on the HEAD `e7639f0` library (previously ~1 in 3 under load) | 13A-latency (the known 13A-5b-iii-b-1 load residual; not chased here) |
| Byte masks/channels and typed planes onto tiles; per-tile mask caches | 13B-4 |
| Measured history/snapshot sharing and budget re-measure | 13B-5 |
| Byte-bounded shared resident-cache registry | 13B-6 |
| The 16 handed 13A items | 13B-7 / 13B-8 / 13B-9 (table above) |
| Recovery / `.rzsnapshot` on-disk tile sharing, spilling, compression | 13C |
| Regional render latency regression (13B-2c-1: `regional_window` warm 1.06–1.10 × with per-sample tile reads, cold 0.77 ×) | 13A-latency (not re-measured here) |
| `verify_phase9h_ui.py` `current-layer-commands-ui` on an unlocked screen (locked during 13B-1, 13B-2a, 13B-2b, 13B-2c-1 and 13B-2c-2a: `CGSSessionScreenIsLocked = true`) — PASSED in 13B-2c-2b on an unlocked screen (56/56 groups); a later FAIL with the screen locked is the same known lock-state failure | closed (13B-2c-2b) |
| The seeded lend's second copy: on an image built by a still-seeded constructor, while its seed is cached, the private lend is a second `w·h·bpp` beside the evictable seed (`peak_host_intake` `lend/seeded_u16` 600×520 2496728 → 4992768 bytes, within its declared HEAD + `w·h·8` + 64; 13B-3e-4a D4) — closes as 4b / 4c drop the seeds; since 13B-3e-4b only the two kept preview seeds (`live_filter_evaluate.rs:429`, `raw_source_geometry.rs:470`) and test-built images (the seeded `from_rgba16` / `from_rgba32_extended` called from `core/tests` and the example) still seed, so only those can double; f32 half CLOSED in 13B-3e-4c-2a (no f32 constructor seeds: `peak_f32_constructors` `lend/seeded_f32` 600×520 held 14976848 → 9984584 bytes, the views 4992000 → 0) | 13A-latency (the two kept preview seeds, `live_filter_evaluate.rs:429` / `raw_source_geometry.rs:471`, since 13B-3e-4c-1) |
| The first preview of a fresh unseeded plane (an intake or a develop output, 13B-3e-4a) materializes the view HEAD's constructor adopted: `peak_host_intake` `pipeline/intake_preview` `held` unchanged (5142683); `typed/raw_intake` `pipeline_preview_fit` warm 74.92 → 85.41 ms, 1.140 ×, recorded (no bound; load ~5–9); later previews hit the cache (the `typed/raw_develop` preview lines ≤ 1.002 ×). 13B-3e-4b adds the decoded planes: a reopened u16 master's and an opened PNG16's first develop preview materializes the view the decoder's seed used to supply — `typed/decoders` (24 MP, load ~9–26) `reopen_rz16_master_preview` `ms_first` 412.03 → 434.97 ms, warm 340.84 → 354.48 ms (1.040 ×, bound met), views 576000000 → 192000000; `open_png16_preview` `ms_first` 358.35 → 369.87 ms, warm 326.91 → 331.85 ms (1.015 ×), views 192000000 → 192000000; `peak_decoders` (600×520) `pipeline/png16_preview` `held` unchanged (5081483), `pipeline/rz16_master_preview` 15077282 → 10085202 | 13A-latency (the read is `preview_view_rgba16`, the kept exception, since 13B-3e-4c-1) |
| The 9h Move probe's lend-pointer oracle (`scripts/native-paint-probe/geometrygestures.swift` compared two handles' `pixels_rgba16` pointers, equal at HEAD only through the cache-keyed view) — CLOSED in 13B-3e-4a (D6: `rz_precision_image_storage_identity`; `precision-move-handles-ui` PASSED in the full 9h run and alone) | closed (13B-3e-4a) |
| Carried nit 5 (13B-3e-4b review): `precision_image_build.rs` is 616 lines (above the ~600 soft cap); `Tiling::Seeded` stays with the kept preview seeds (13B-3e-4c-1), so the shrink rides on 4c-2's `tile()` f32 seed / `OutputView` work — CLOSED in 13B-3e-4c-2a (616 → 594 lines: the f32 `tile()` arm and `from_rgba32_extended_unseeded` deleted) | closed (13B-3e-4c-2a) |
