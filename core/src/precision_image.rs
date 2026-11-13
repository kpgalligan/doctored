//! Phase 7A's isolated storage experiment, not an editor document mode.
//! Owned straight RGBA16 UNORM and normalized RGBA32 float share one encoded
//! RGB/profile contract. Hidden RGB is preserved in sources and explicit
//! conversions; compositing has no observable color at alpha zero. HDR and
//! profile conversion refuse.
//!
//! REPRESENTATION (13B-3b-1, the typed twin of 13B-2b's `LayerRaster`). The
//! samples are private and one of two:
//!
//! - **Tiled** — a `tile_store::TilePlane<[u16; 4]>` / `<[f32; 4]>`. Every
//!   constructor builds the image over the adopted `Arc<Vec<_>>` exactly as
//!   13B-3a did (admission, reservation and validation read that buffer),
//!   then tiles it: the plane is copied into 256×256 tiles. Only the seeded
//!   u16 `from_rgba16` (`tile()`) also SEEDS the process-wide view cache
//!   (`contiguous_views`) with the adopted buffer; since 13B-3e-4c-2a no f32
//!   constructor seeds. Tiling polls nothing, so no checkpoint sequence
//!   moves.
//! - **Contiguous** — 13B-3a's `Arc<Vec<_>>`, kept only when the tile store
//!   refuses the plane (an allocation failure); since 13B-3e-4c-1 no f32
//!   whole-view reader exists. Every plane built without an adopted `Vec`
//!   is tiled at every size (filled planes, uniform decoded planes, the
//!   float `.rz` reopen and float TIFF open, decoded straight into tiles,
//!   and every `with_rect` output), and every adopted f32 plane is too,
//!   above `CONTIGUOUS_CACHE_BYTES` (67,108,864 f32 px, reachable at the
//!   8 GiB limit of a 16 GB Mac) included: each adopting plan charges the
//!   tiling copy beside its `Vec`.
//!
//! ACCESSOR CONTRACT (13B-3a). There is NO borrowed-slice read accessor, no
//! `Deref`, no `PartialEq` and no `AsRef<[_]>`, and no public whole-plane
//! view: since 13B-3e-4c-1 the ONE view read left is the crate-private
//! `preview_view_rgba16`, the exception kept by decision (13B-3e-4c-1 D2), bound ONCE
//! per call by the Develop preview (`develop_inspect.rs`; its other caller,
//! the live-filter preview, was removed on 2026-10-10); a miss is a
//! whole-plane materialization,
//! cached. Since 13B-3c-1 the READS go through the tiles and bind
//! nothing: rows through `row_segments` (borrowed `PrecisionRun`s), one
//! sample through `sample_at`, a sequential whole-plane walk through
//! `runs_rgba16` / `runs_rgba32` — the typed twins of `LayerRaster`'s
//! 13B-2c-1 primitives, a u16 sample normalized by the one `normalized16`
//! rule; since 13B-3c-2a one STORED u16 code through `rgba16_at` (no
//! normalization, `None` for f32 or outside), which the projection-handle
//! consumers (stats sample, scoped analysis) read; `pixel(index)` stays only
//! where an index is all a writer has (13B-3d's callers). `to_rgba16` /
//! `to_rgba32` are owned copies; since 13B-3c-2c-1 a tap walk (one
//! random-access read per operation) binds `transient_rgba16` /
//! `transient_rgba32` (a contiguous plane borrowed, a tiled one copied; no
//! view read or written). IDENTITY is `ptr_eq` — a private `Arc<()>`
//! token minted by every constructor and shared by clones, `with_profile`
//! and a same-storage `convert`, never the samples; SAMPLES are
//! `same_samples` (no allocation, no job, f32 compared bitwise, tiles through
//! `TilePlane::same_bits`). The residual still to be moved onto tile reads is
//! `rg 'contiguous_rgba(16|32)\('` (15 call lines after 13B-3e-1; 13 after
//! 13B-3e-2; 7 after 13B-3e-3a; 5 after 13B-3e-3b; 2 after 13B-3e-4a; 0
//! after 13B-3e-4c-1, the accessor removed).
//!
//! THE FFI LEND. `pinned_rgba16` pins a PRIVATE copy of a tiled u16 plane
//! (`Plane::lent`; a contiguous plane lends its own buffer) once and stores
//! it in the image itself, so the pointer the FFI export
//! `dr_precision_image_pixels_rgba16` lends lives exactly as long as the
//! HANDLE. f32 storage has no lend; `to_rgba32` is its owned
//! copy. Since 13B-3e-4a the lend never reads or writes the view cache, and
//! a copy that cannot be allocated answers `None` (the FFI's NULL) rather
//! than panicking. A clone starts with no pinned copy. It is a lend for the
//! FFI and never a `core/src` read path. A pinned copy is
//! reported as `precision_views` (retained). The lend's pointer is
//! per-handle and never an identity; `plane_key` /
//! `dr_precision_image_storage_identity` is the identity.
//!
//! WRITERS (13B-3b-2, `LayerRaster`'s 13B-2b contract). `rebuilt_rgba16` /
//! `rebuilt_rgba32_extended` build the image a whole-image edit answers over
//! the edited image's tiles, sharing every bitwise-unchanged one (a fresh
//! identity even for equal samples; since 13B-3d-3a they seed nothing);
//! `with_rect_rgba16/32` overwrite one rect (untouched tiles shared, NO
//! seed); `read_rect_rgba16/32` copy one rect (no view);
//! `from_rgba16_unseeded` tiles without seeding (promotion, placeholders,
//! and since 13B-3d-3a the layer producers: the artwork render, the warp
//! output, Rasterize, the paste conversion, the retouch new layer);
//! since 13B-3c-2b `from_float_samples_unseeded` /
//! `from_rendered_float_unseeded` are the render constructors' unseeded
//! twins (the same statements, no seed; since 13B-3e-4b the only render
//! constructors, their seeded twins removed with the seeded output view,
//! and since 13B-3e-4c-2a no render or resample takes an output view);
//! `reidentified` is the same
//! planes under a fresh identity (a render that places its source
//! unchanged). Since 13B-3c-2c-2 the Copy / Cut payload is unseeded at
//! both storages (the f32 one through `from_rgba32_extended`, which seeds
//! nothing since 13B-3e-4c-2a) and `convert`'s cross-storage output is
//! unseeded. Since 13B-3d-2 the
//! typed writers bind no whole view: rect writers (paint, retouch in place,
//! the lock restore over `unshared_rect`) or runs + `rebuilt_*` (incl.
//! `rebuilt_float_samples`). Since 13B-3d-3a every typed LAYER producer is
//! unseeded — the merges, stamp, flatten and members render, the bake, the
//! float→u16 and narrowing tone maps and the constructors above — so a
//! whole reader of the installed layer pays one materialization on its
//! first read. Since 13B-3d-3b `filled_rgba16` / `filled_rgba32_extended`
//! build one repeated sample over `TilePlane::filled` (one tile per shape,
//! no view). Since 13B-3e-4b `from_rgba16_decoded` /
//! `from_rgba32_extended_decoded` build every reader-side decoder's plane
//! (the typed `.rz` layer / master PNG, the WARP original, the f32 `.rz`
//! raster, the PNG16 / TIFF16 / float TIFF open, the 16-bit PSD layer and
//! composite, the RZCB / RGBA8 payloads) with the unseeded statements, a
//! bitwise-uniform plane filled (`plane::uniform_tiles`), so no render or
//! decoder seeds the view cache; the seeded `from_rgba16` remains for
//! test-built images and the one kept preview seed
//! (`raw_source_geometry.rs`, feeding `preview_view_rgba16`). Since 13B-3e-4c-2a no f32 constructor seeds:
//! `from_rgba32` / `from_rgba32_extended` tile unseeded. The
//! representation lives in the `#[path]` children `plane`
//! (`precision_image_plane.rs`), `build` (`precision_image_build.rs`) and
//! `fill` (`precision_image_fill.rs`), which alone see the private fields.
//!
//! Immutable straight RGBA16/RGBA32 normalized sources carry one captured profile; Normal raster,
//! supported groups, u16 masks and live adjustments share `blend.rs` with f32 intermediates and
//! explicit output conversion. Checked alpha products retain positive-underflow provenance and the
//! qualified 2^-96 floor. Bounded work, unique allocation accounting and single-use cancellation;
//! unsupported operations and extremely small effective alpha refuse. The 7A representation
//! decision and later qualification live in `designs/phase-7-high-depth-status.md`. **Accessor rule
//! (13B-3a):** no whole-plane view is read — samples go through the tile reads or an owned
//! `to_rgba16/32` copy; the ONE named exception is the crate-private `preview_view_rgba16`, kept
//! exception by decision (13B-3e-4c-1 D2), bound ONCE per call by the Develop preview
//! (`develop_inspect.rs`) — never inside a per-row or per-sample
//! loop or closure. **Since 13B-3c-2c-1** a tap walk (ONE random-access read per operation —
//! `precision_sampling`'s resample inputs, the retouch source's bilinear tap walks) binds
//! `transient_rgba16` / `transient_rgba32` (`pub(crate)`, a `Cow`: a contiguous plane borrowed, a
//! tiled one a transient owned copy; no view read or written, nothing polled; a failed copy answers
//! Err "could not allocate precision output" where the view materialization panicked), after an
//! explicit `storage()` gate carrying HEAD's refusal sentence. The typed compositor's raster arm,
//! `composite_pixels_into`, `typed_shape` and the knockout unit pass read rows
//! (`StyledPixels::visit_row`), the regional tree `sample_at` (`StyledPixels::sample_at` /
//! `alpha16_code`), and `precision_geometry::image_equal` walks `runs_rgba16/32` (same
//! checkpoints), so a projection or a regional window leaves no view over the document. **Since
//! 13B-3c-2a** one STORED u16 code reads through `rgba16_at(x, y)` (`Plane::sample_at`, no
//! normalization, `None` for f32 or outside, no view), and every consumer of a projection handle
//! reads the tiles with its checkpoints, reports and refusal sentences unchanged:
//! `quantize_with_context` walks `row_segments` rows (flat `i = y·w + x` for its `i & 4095` report,
//! Bayer rank `(y & 7)·8 + (x & 7)`) and `quantize_scratch_bytes` answers from the dimensions;
//! `tone_mapped_u16` feeds `runs_rgba32().flatten()`; `precision_stats` / `float_stats` sample
//! through `rgba16_at` / `sample_at` and histogram in TILE-BAND order over `row_segments` (the
//! float histogram is the scoped analysis's flat `sample_at` walk) (each 256-row band,
//! each 256-column span; the flat indices that are multiples of `step`, the report hoisted to each
//! 4096-sample boundary at `i = visited·step` — order-independent counters, so the flat walk's
//! answers and reports); `proof_render16_with` walks `runs_rgba16().flatten()` with `job.every(i)`;
//! `dr_precision_image_compare` pairs both images' runs with two cursors (split at the shorter run)
//! and accumulates the slice zips in nested loops into `difference_json`'s `Difference` counters;
//! `scoped_analysis`'s typed closure reads `rgba16_at` through `mapped_xy` — so none binds a view
//! (the projection OUTPUT's own seed is 13B-3c-2b's). **Since 13B-3c-2b** every typed render call
//! site named an output view (a `pub(crate)` enum in `precision_render.rs`: seeded — the adopted
//! buffer seeding the view cache because an export chain bound the output whole in the same
//! operation (`doc_export`, `doc_export_layers`, `precision_io`'s save; no caller since 13B-3e-1;
//! removed with the seeded render constructors `from_float_samples` / `from_rendered_float` in
//! 13B-3e-4b) — or unseeded, tiles only; unrelated to `KnockoutSeed`, the "seeded" in
//! `render_seeded_with_context`); **since 13B-3e-4c-2a the type and all nine of its `view`
//! parameters are gone: renders, tone maps and resamples take no output-view argument and are
//! always unseeded.** `PrecisionScene::render_with_context`,
//! `PrecisionDocument::render_with_context` / `render_cut_with_context` and `analysis_source` build
//! through `from_float_samples_unseeded` / `from_rendered_float_unseeded` (the same statements
//! through the private `Tiling`), as do the public `PrecisionScene::render` /
//! `PrecisionDocument::render`. UNSEEDED: the host projection (`render`; its whole-plane readers,
//! the exports, pay ONE materialization per handle), the regional window (always the unseeded
//! pair), the quantize / tone-map conversions (`precision_conversion`, `float_document`'s
//! float→u16), Copy / Copy Merged (`Window::read` walks `row_segments` rows, zeros where the source
//! does not reach), both conversion previews (the narrow one pairs runs through `paired_runs`, the
//! structure one walks runs against `as_raw().chunks_exact(4)`), `scoped_analysis`'s source and the
//! preflight's `baked_projection` (a `runs_rgba16` opacity scan), and since 13B-3c-2c-1 the retouch
//! PLATES (the Current arm copies the layer's rows into `from_rgba16_unseeded`; the cut render;
//! their readers take rows — the host's quantize, the new-layer backdrop, the inpainter's window —
//! or one transient — the source transform, whose output is unseeded too, and the clone/heal
//! source) and the SELECTION SOURCE (`analysis_source`, copied onto the canvas by rows in
//! `canvas_samples` and dropped before the analysis). Since 13B-3c-2c-2 the Copy / Cut PAYLOAD is
//! UNSEEDED too (`Window::u16` → `from_rgba16_unseeded`, `Window::f32` → `from_rgba32_extended`,
//! through a `pub(crate)` unseeded twin until 13B-3e-4c-2a, when no f32 constructor seeds any more
//! and the twin was deleted; its sequential readers, the RZCB `encode` and `converted`, walk
//! `runs_rgba16/32` with HEAD's bytes and checkpoints), and so is `convert`'s cross-storage output
//! (`from_float_samples_unseeded`); every other constructor site stays Seeded with its readers and
//! flip owner named (`designs/phase-13b-tiled-shipped.md`, 13B-3c-2c-2 decisions).
//! `tone_mapped_u16` took a `view` as well (none since 13B-3e-4c-2a, always unseeded): unseeded for
//! `display_rgba8`, `downconvert`, the narrow preview, the byte narrowing and, since 13B-3d-3a,
//! `float_to_u16` and the u16 narrowing (the layer they install is read through tiles), and
//! `export_stages` seeded only when the export kept the source's size (else its only reader was the
//! resize's transient; unseeded always since 13B-3e-1). **Since 13B-3d-3a every typed LAYER
//! producer is UNSEEDED** — the merges / stamp / flatten and `render_seeded_with_context` (the
//! members render), the bake's `baked_projection` (read by `convert_pixels`' runs), the artwork
//! render, the warp output, Rasterize, the converted paste, the retouch new layer
//! (`from_rgba16_unseeded`), `float_to_u16` / the u16 narrowing and every `rebuilt_*` answer: each
//! is installed as a layer whose view-free readers (the rect / runs writers, the compositor, the
//! regional window, thumbnails, histograms) take tiles, and whose whole readers —
//! `precision_warp`'s source read, `warp_native`, `selected_filter`, `selected_auto_levels`,
//! `precision_plane_read` (and `precision_cutout`'s foreground scan) — read tiles since 13B-3e-3a
//! (TILE-BAND where order-independent, runs row-major where not; the warp's taps ONE
//! `transient_rgba16` per call), so none materializes a view (`tests/precision_layer_producers.rs`,
//! `tests/precision_whole_readers.rs`; `rg` of the seeded constructor / seeded-render sites outside
//! `precision_image*` 47 → 35 lines). SEEDED, each reader named: the export renders (`doc_export`,
//! `doc_export_layers`, `precision_io`'s save; 13B-3e), the export resize (`doc_export.rs:1316`,
//! read whole by the sharpen and `convert16`; 13B-3e) and the style bake (`precision_style_render`,
//! untouched; 13B-3e) — all UNSEEDED since 13B-3e-1 (below). **Since 13B-3d-3b** the resample
//! outputs took their CALLER's output view (`transform_image` / `resize_image`'s `view` and a
//! private u16 / f32 dispatch, all removed in 13B-3e-4c-2a: the outputs are `from_rgba16_unseeded`
//! / `from_rgba32_extended`, always unseeded): the geometry outputs (`precision_geometry`), the
//! placed source's Sample arm (`retained_source_render`) and `develop_detail` were unseeded, only
//! `doc_export.rs:1316` seeded (unseeded since 13B-3e-1); the FILLED constructors `filled_rgba16` /
//! `filled_rgba32_extended` (`pub(crate)`, child `fill`) build one repeated sample over
//! `TilePlane::filled` — one tile per shape plus the index, no view, no control, no admission
//! (refusals: `pixel_count`'s sentence, `EXTENDED`, `ALLOCATION`; tiled at every size) — for
//! `add_layer_at` and the promotion plan's sourced shapes, so an untouched added
//! layer retains ≤ 4 tiles; and `precision_codec::encode` walks `runs_rgba16` with
//! HEAD's `job.every(i)` sequence, so the typed native save, PNG16 and TIFF16 bind no view.
//! `ptr_eq` for identity (same storage and same allocation), `same_samples` for samples (no
//! allocation, no job, f32 bitwise via `Sample::same_bits`), `to_rgba16/32` for an owned copy.
//! **Since 13B-3b-1** it is `Tiled(TilePlane<[u16;4]> / <[f32;4]>)` or `Contiguous(Arc<Vec<_>>)` (a
//! plane the tile store refuses; every f32 plane tiles at every size): every
//! constructor keeps 13B-3a's
//! statement order over the adopted Vec, then `tile()`s it (no job, no checkpoint) and SEEDS the shared view
//! cache (`contiguous_views.rs`) with that Vec — since 13B-3e-4c-2a only a u16 plane (the seeded
//! `from_rgba16`). `storage_bytes` / `precision_pixels` are `w·h·bpp` tiled (the index is
//! `tile_structure`, `precision_buffer` only for a contiguous fallback); `plane_key()` is the dedup
//! key `precision_color` uses, since 13B-3e-4a also the storage identity
//! `dr_precision_image_storage_identity` exports (equal ⇒ one tile index or buffer, nothing copied;
//! the 9h Move probe's oracle), because the lend's pointer is per-handle. The FFI pointer export
//! `dr_precision_image_pixels_rgba16` lends `pinned_rgba16()` (f32 storage has no lend;
//! `to_rgba32` is its copy), stored in the image (`OnceLock`, a clone starts empty; reported as
//! retained `precision_views`), so the HANDLE owns what it lends — a lend, never a `core/src` read
//! path; since 13B-3e-4a it is a PRIVATE copy (`Plane::lent`: a tiled plane's `to_contiguous`, a
//! contiguous plane's own `Arc`) that never reads or writes the view cache, and a failed copy
//! answers `None` (the FFI's NULL) rather than panicking. The residual still to move onto tile
//! reads is `rg 'contiguous_rgba(16|32)\('` (72 call lines after 13B-3b-1 and after 13B-3b-2; 69
//! after 13B-3c-1; 59 after 13B-3c-2a; 53 after 13B-3c-2b; 45 after 13B-3c-2c-1; 41 after
//! 13B-3c-2c-2; 36 after 13B-3d-1; 24 after 13B-3d-2 and after 13B-3d-3a, whose flips drop output
//! seeds and move no read — `rg -c` over `core/src` stays 26 with the accessor definitions; 23
//! after 13B-3d-3b, `rg -c` 25; 15 after 13B-3e-1, `rg -c` 17; 13 after 13B-3e-2, `rg -c` 15; 7
//! after 13B-3e-3a, `rg -c` 9; 5 after 13B-3e-3b, `rg -c` 7 — the lend, the two kept preview reads
//! (13B-3e-2's `live_filter_preview.rs:164`, 13B-3e-3b's `develop_inspect.rs:136`) and the geometry
//! source bind `raw_source_geometry.rs:392`; 2 after 13B-3e-4a, `rg -c` 4 — the two kept preview
//! reads; 0 after 13B-3e-4c-1, `rg -c` 0 — the accessor removed, the two reads on
//! `preview_view_rgba16`, kept exception by decision (13B-3e-4c-1 D2); one read since
//! 2026-10-10, when the live-filter preview was removed). **Since 13B-3e-4b**
//! every reader-side typed DECODER — the typed `.rz` layer / master PNG (`precision_png`), the WARP
//! original (`warp_native`), the f32 `.rz` raster (`float_native`), the PNG16 / TIFF16 open
//! (`precision_codec`), the float TIFF open (`float_io`), the 16-bit PSD layer and composite
//! (`psd_import16`) and the RZCB / RGBA8 clipboard payloads (`precision_clipboard_codec`) — builds
//! through `from_rgba16_decoded` / `from_rgba32_extended_decoded` (`pub(crate)`: the unseeded
//! statements through the private `Tiling::Decoded`, and a bitwise-uniform plane — a reopened empty
//! layer, a pixel-less PSD record — becomes ONE filled plane through `plane::uniform_tiles`), and
//! the narrowing's base raster, WARP original and 1×1 placeholders are `LayerRaster::unseeded`, so
//! an open, a reopen, a paste or a narrowing seeds no view; the seeded constructor site left
//! outside `precision_image*` is in `raw_source_geometry.rs`,
//! feeding `preview_view_rgba16`, kept exception by decision (13B-3e-4c-1 D2). **Since
//! 13B-3e-1** the typed EXPORT chain, the 16-bit PSD writer and the float writers read tiles and
//! seed nothing: the export renders (`export_encoded`, `export_layer_encoded`),
//! `export_stages`' tone map and resize, the `convert16` / sharpen /
//! trim-crop outputs (`from_rgba16_unseeded`) and the style bake (`bake_style16`,
//! `from_float_samples_unseeded`) are UNSEEDED; `convert16` walks `runs_rgba16` with its
//! `job.every(i)` sequence, the sharpen feeds the resampled runs to
//! `ops_photographic::unsharp_mask_runs` (`unsharp_mask_samples` delegates to it), the trim scans
//! alpha in TILE-BAND order (`alpha_bounds16(image, job)`, one check per band, the private
//! `Extent`) and crops ONE `read_rect_rgba16` (`crop_rect`, one `read_rect_rgba16/32`);
//! `psd_export16` reads the layer / flattened channels through its private `channels16` (the four
//! planes from ONE runs walk per image) after `storage()` gates carrying HEAD's sentences, and
//! `composite_planes16` takes the projection (one runs pass); `float_io::encode_float_tiff` and
//! `float_native::put_raster` walk `runs_rgba32` with HEAD's checkpoints (4096-sample chunks), so
//! no export, PSD save or float save binds a view (`tests/precision_export_reads.rs`). The WRITER
//! contract is `LayerRaster`'s (13B-2b): `rebuilt_rgba16` / `rebuilt_rgba32_extended` (the
//! constructor's length check, admission and validation, no job; over a same-storage tiled image of
//! equal dimensions every bitwise-unchanged tile is shared through `TilePlane::rebuilt_from`, else
//! fresh tiles; since 13B-3d-3a NOTHING is seeded — a rebuild that cannot share tiles tiles fresh
//! and unseeded; ALWAYS a fresh identity) serve the whole-image typed writers
//! (`precision_scalar_edit`, `precision_mask_apply`, `precision_retouch`'s new-layer / grown
//! extent, `selected_adjustment`, `precision_color`, `precision_color_policy`, and
//! `precision_paint_tools`' GROWN extent), which since 13B-3d-2 read their input as `runs_rgba16`
//! in plain nested loops (the index taken at the top), and `rebuilt_float_samples`
//! (`from_float_samples_unseeded`'s statements over the edited image's tiles) serves the
//! destructive adjustment, whose changed scan pairs both images' runs with two cursors;
//! `with_rect_rgba16/32` (`TilePlane::with_rect` semantics, `Ok(None)` for a zero area or no
//! bitwise change, untouched tiles shared, NO seed; f32 also refuses a sample failing
//! `extended_pixel`) and `read_rect_rgba16/32` (no view) landed with their suite, and **since
//! 13B-3d-1 the typed PAINT writers are rect writers** on them — 7D `paint` / `erase` / `clone` /
//! `heal`, the five native tools and the float Cut's erase read ONE rect (the coverage's bbox on
//! the layer through `layer_rect`'s plane forms, the heal window on the layer, a native tool's
//! coverage rect) with their per-pixel bodies, refusals and plan JSON unchanged, and write it back
//! through `with_rect` (whose bitwise diff is the changed latch), so a typed dab keeps ≤ 4 new
//! tiles, walks no whole plane and leaves no view (`tests/precision_rect_writers.rs`); **since
//! 13B-3d-2** the in-place retouch arm and the transparency-lock restore are rect writers too —
//! `precision_lock::freezing_alpha` restores over `unshared_rect` (`pub(crate)`, no allocation: the
//! bbox of the tile rects `dst` does not share with `src` by pointer, `None` when all are shared,
//! the whole plane on a storage / dimension mismatch or a contiguous pair), one `read_rect_*` of
//! each written back into `src` through `with_rect_*` (`None` → `src.reidentified()`), exact
//! because a shared tile has `d == s` bitwise (`tests/precision_retouch_writers.rs`);
//! `from_rgba16_unseeded` (the same statements, no seed) builds the 16-bit structure promotion's
//! planes and the 1×1 placeholders; `reidentified` (`pub(crate)`, same planes, fresh identity) is
//! `render_high_depth`'s Shared arm, so a typed identity placement shares every tile of its master;
//! `shared_tile_count` is the test oracle.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use image::RgbaImage;

use crate::contiguous_views::{self, PlaneKey, View};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::resources::Inventory;

#[path = "precision_image_build.rs"]
mod build;
pub(crate) use build::unit_to_u16;
#[path = "precision_image_fill.rs"]
mod fill;
#[path = "precision_image_plane.rs"]
mod plane;
use plane::{Pixels, Plane};

/// The deterministic test pin, and the process limit's initial value until
/// the host sets the RAM-scaled one (`dr_set_process_working_limit`).
pub const REFERENCE_WORKING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The process-wide working limit, read wherever no caller limit is passed
/// (internal constructors, `*_plan()` defaults, the native reopen check, the
/// PSD layered-import envelope). The host sets it once at launch.
static PROCESS_WORKING_LIMIT: AtomicU64 = AtomicU64::new(REFERENCE_WORKING_BYTES);

/// The process-wide working limit (initially [`REFERENCE_WORKING_BYTES`]).
pub fn process_working_limit() -> u64 {
    PROCESS_WORKING_LIMIT.load(Ordering::Relaxed)
}

/// One cache bound: the process working limit / 16 (`cache_limit.rs`; 128 MiB
/// at the pin, 512 MiB at 8 GiB).
pub fn process_cache_limit() -> u64 {
    crate::cache_limit::cache_limit(process_working_limit())
}

/// The host's post-projection trim threshold for non-view cache bytes: two
/// cache bounds, the process working limit / 8 (`cache_limit.rs`).
pub fn process_cache_trim_limit() -> u64 {
    crate::cache_limit::cache_trim_limit(process_working_limit())
}

/// Sets the process-wide working limit. `false` (value unchanged) for 0.
pub fn set_process_working_limit(bytes: u64) -> bool {
    if bytes == 0 {
        return false;
    }
    PROCESS_WORKING_LIMIT.store(bytes, Ordering::Relaxed);
    true
}

/// `bytes` in GiB for a refusal sentence: whole GiB as `"2"`, else one
/// decimal as `"2.7"`.
pub(crate) fn gib_text(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes.is_multiple_of(GIB) {
        format!("{}", bytes / GIB)
    } else {
        format!("{:.1}", bytes as f64 / GIB as f64)
    }
}
pub(crate) const BOOKKEEPING_BYTES: u64 = 1024 * 1024;

/// The allocation-failure sentence of a precision copy or view.
const ALLOCATION: &str = "could not allocate precision output";

/// The refusal of every direct RGBA8 reading of f32 storage (`to_rgba8`, the
/// layer thumbnail): float display goes through the document's tone map.
pub(crate) const FLOAT_RGBA8_REFUSAL: &str =
    "a 32-bit float image is displayed through its document's tone map, not by direct RGBA8 conversion";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecisionStorage {
    Rgba16Unorm,
    Rgba32Float,
}

impl PrecisionStorage {
    pub const fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Rgba16Unorm => 8,
            Self::Rgba32Float => 16,
        }
    }
}

pub struct PrecisionImage {
    width: u32,
    height: u32,
    profile: Arc<IccProfile>,
    pixels: Pixels,
    identity: Arc<()>,
    pinned: OnceLock<Arc<Vec<[u16; 4]>>>,
    /// The flat-save list's alpha facts of `pixels` (`alpha_facts_once`).
    alpha: Arc<OnceLock<(bool, bool)>>,
}

/// Shares the samples, the profile, the identity and the alpha facts (a fact
/// about the shared samples, so every clone may answer it); the clone starts
/// with NO pinned copy (a handle owns the private copy the FFI lend pins,
/// module doc).
impl Clone for PrecisionImage {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            profile: self.profile.clone(),
            pixels: self.pixels.clone(),
            identity: self.identity.clone(),
            pinned: OnceLock::new(),
            alpha: Arc::clone(&self.alpha),
        }
    }
}

/// One borrowed run of a [`PrecisionImage`] row, in the image's storage.
#[derive(Clone, Copy)]
pub enum PrecisionRun<'a> {
    Rgba16(&'a [[u16; 4]]),
    Rgba32(&'a [[f32; 4]]),
}

impl PrecisionRun<'_> {
    pub fn len(&self) -> usize {
        match self {
            Self::Rgba16(run) => run.len(),
            Self::Rgba32(run) => run.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Sample `i` of the run as [`PrecisionImage::pixel`] answers it (u16
    /// through the one `/ 65535` rule, f32 verbatim). Panics out of range,
    /// as a slice index does.
    pub fn pixel(&self, i: usize) -> [f32; 4] {
        match self {
            Self::Rgba16(run) => plane::normalized16(run[i]),
            Self::Rgba32(run) => run[i],
        }
    }
}

/// The runs of one row span of a [`PrecisionImage`]
/// (`PrecisionImage::row_segments`).
pub struct PrecisionRowSegments<'a> {
    inner: Rows<'a>,
}

enum Rows<'a> {
    U16(plane::PlaneRows<'a, [u16; 4]>),
    F32(plane::PlaneRows<'a, [f32; 4]>),
}

impl<'a> Iterator for PrecisionRowSegments<'a> {
    type Item = (u32, PrecisionRun<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            Rows::U16(runs) => runs.next().map(|(x, run)| (x, PrecisionRun::Rgba16(run))),
            Rows::F32(runs) => runs.next().map(|(x, run)| (x, PrecisionRun::Rgba32(run))),
        }
    }
}

pub(crate) fn pixel_count(width: u32, height: u32) -> Result<usize, String> {
    let count = u64::from(width) * u64::from(height);
    if count == 0 || count > crate::doc::MAX_PIXELS {
        return Err("precision prototype dimensions exceed the supported pixel limit".into());
    }
    usize::try_from(count).map_err(|_| "precision pixel count exceeds addressable memory".into())
}

/// The working memory of [`PrecisionImage::to_rgba8`] over a `width` x
/// `height` image: bookkeeping plus the 4-byte-a-pixel RGBA8 output. The ONE
/// statement of that charge, read by `to_rgba8` and the display estimate.
pub(crate) fn rgba8_preview_scratch(width: u32, height: u32) -> Result<u64, String> {
    let count = pixel_count(width, height)? as u64;
    count
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| "preview working-memory calculation overflow".to_string())
}

pub(crate) fn admit(retained: u64, scratch: u64, other: u64, limit: u64) -> Result<(), String> {
    if limit == 0 {
        return Err("precision working limit must be positive".into());
    }
    let total = retained
        .checked_add(other)
        .and_then(|v| v.checked_add(scratch))
        .ok_or("precision working-memory calculation overflow")?;
    if total > limit {
        return Err(format!("precision operation needs {total} bytes including retained inputs; working limit is {limit} bytes"));
    }
    Ok(())
}

/// The extended-range storage invariant, written once: finite RGB within
/// ±`FLOAT_RANGE_MAX` and alpha in [0, 1] (`float_document`'s contract).
pub(crate) fn extended_pixel(pixel: [f32; 4]) -> bool {
    let max = crate::float_document::FLOAT_RANGE_MAX;
    pixel[..3].iter().all(|v| v.is_finite() && v.abs() <= max)
        && pixel[3].is_finite()
        && (0.0..=1.0).contains(&pixel[3])
}

pub(crate) fn reserved_vec<T>(count: usize) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| "could not allocate precision output".to_string())?;
    Ok(out)
}

impl PrecisionImage {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn profile(&self) -> &Arc<IccProfile> {
        &self.profile
    }
    /// Change interpretation only; the immutable samples (and the identity)
    /// are shared.
    pub(crate) fn with_profile(&self, profile: Arc<IccProfile>) -> Self {
        Self {
            profile,
            ..self.clone()
        }
    }
    pub fn storage(&self) -> PrecisionStorage {
        match self.pixels {
            Pixels::U16(_) => PrecisionStorage::Rgba16Unorm,
            Pixels::F32(_) => PrecisionStorage::Rgba32Float,
        }
    }
    /// The ONE whole-plane view read left: an exception kept by decision
    /// (13B-3e-4c-1 D2). Its only caller is
    /// `PrecisionImage::develop_preview` (`develop_inspect.rs`), which
    /// binds it ONCE per call; its feeding seed is in
    /// `raw_source_geometry.rs`. Tiled:
    /// the cached view, a miss materializing and caching it (panicking
    /// "could not allocate precision output" when it cannot be allocated,
    /// as before); contiguous: the image's own `Arc`; `None` for RGBA32
    /// float storage. Every other read goes through tiles (`row_segments`,
    /// `sample_at`, `rgba16_at`, `runs_rgba16/32`).
    pub(crate) fn preview_view_rgba16(&self) -> Option<Arc<Vec<[u16; 4]>>> {
        match &self.pixels {
            Pixels::U16(Plane::Tiled(plane)) => Some(
                contiguous_views::view(PlaneKey::Rgba16(plane.weak_index()), || {
                    View::Rgba16(Arc::new(plane.to_contiguous(None).expect(ALLOCATION)))
                })
                .into_rgba16()
                .expect("a u16 plane's view is u16"),
            ),
            Pixels::U16(Plane::Contiguous(p)) => Some(Arc::clone(p)),
            Pixels::F32(_) => None,
        }
    }
    /// The FFI LEND of the u16 samples (module doc): a private copy of a
    /// tiled plane (a contiguous plane lends its own buffer) made once,
    /// stored in this image and answered from it afterwards, so the slice
    /// lives as long as `self` (a racing loser drops its copy). Never reads
    /// or writes the view cache. `None` for f32 storage or when the copy
    /// cannot be allocated (the FFI's NULL). Never a
    /// `core/src` read path — read through `row_segments`, `runs_rgba16` or
    /// `transient_rgba16` instead.
    pub fn pinned_rgba16(&self) -> Option<&[[u16; 4]]> {
        let Pixels::U16(plane) = &self.pixels else {
            return None;
        };
        if self.pinned.get().is_none() {
            let lent = plane.lent().ok()?;
            let _ = self.pinned.set(lent);
        }
        self.pinned.get().map(|samples| samples.as_slice())
    }
    /// The flat-save list's alpha facts of these samples
    /// (`precision_preservation::alpha_facts`): `scan` runs on the first ask
    /// and its answer is kept for this image and every clone, which share the
    /// samples. It cannot go stale: nothing writes a built image's samples,
    /// and every writer answers a new image (`assembled`), whose cell starts
    /// empty. A concurrent first ask waits for the one scan.
    pub(crate) fn alpha_facts_once(&self, scan: impl FnOnce() -> (bool, bool)) -> (bool, bool) {
        *self.alpha.get_or_init(scan)
    }
    /// Whether [`Self::alpha_facts_once`] has answered for these samples.
    pub(crate) fn alpha_facts_known(&self) -> bool {
        self.alpha.get().is_some()
    }
    /// The whole u16 plane for ONE random-access read per operation (a tap
    /// walk, a per-index closure): a contiguous plane borrows its own buffer,
    /// a tiled one answers a transient owned copy. Never reads or writes the
    /// view cache; polls nothing. Err "precision image storage is not RGBA16"
    /// for f32, "could not allocate precision output" when the copy fails.
    pub(crate) fn transient_rgba16(&self) -> Result<Cow<'_, [[u16; 4]]>, String> {
        match &self.pixels {
            Pixels::U16(Plane::Tiled(plane)) => plane
                .to_contiguous(None)
                .map(Cow::Owned)
                .map_err(|_| ALLOCATION.to_string()),
            Pixels::U16(Plane::Contiguous(p)) => Ok(Cow::Borrowed(p.as_slice())),
            Pixels::F32(_) => Err("precision image storage is not RGBA16".into()),
        }
    }
    /// The one sample of a TILED u16 plane whose samples are all bitwise
    /// equal (`TilePlane::uniform_sample`), else `None` — always for a
    /// contiguous plane and for f32 storage. Allocates nothing
    /// (`precision_sampling::uniform_source`).
    pub(crate) fn uniform_rgba16(&self) -> Option<[u16; 4]> {
        match &self.pixels {
            Pixels::U16(Plane::Tiled(plane)) => plane.uniform_sample(),
            _ => None,
        }
    }
    /// The f32 twin of [`Self::transient_rgba16`].
    pub(crate) fn transient_rgba32(&self) -> Result<Cow<'_, [[f32; 4]]>, String> {
        match &self.pixels {
            Pixels::F32(Plane::Tiled(plane)) => plane
                .to_contiguous(None)
                .map(Cow::Owned)
                .map_err(|_| ALLOCATION.to_string()),
            Pixels::F32(Plane::Contiguous(p)) => Ok(Cow::Borrowed(p.as_slice())),
            Pixels::U16(_) => Err("precision image storage is not RGBA32 float".into()),
        }
    }
    /// An owned copy of the u16 samples. Err "could not allocate precision
    /// output" when it cannot be allocated.
    pub fn to_rgba16(&self) -> Result<Vec<[u16; 4]>, String> {
        match &self.pixels {
            Pixels::U16(p) => p.to_vec(),
            Pixels::F32(_) => Err("precision image storage is not RGBA16".into()),
        }
    }
    /// An owned copy of the f32 samples.
    pub fn to_rgba32(&self) -> Result<Vec<[f32; 4]>, String> {
        match &self.pixels {
            Pixels::F32(p) => p.to_vec(),
            Pixels::U16(_) => Err("precision image storage is not RGBA32 float".into()),
        }
    }
    /// IDENTITY: the same storage and the same identity token (minted by a
    /// constructor, shared by clones, `with_profile` and a same-storage
    /// `convert`). Equal samples in two constructions answer false.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        a.storage() == b.storage() && Arc::ptr_eq(&a.identity, &b.identity)
    }
    /// Sample EQUALITY with no allocation and no job: false on a storage or
    /// dimension mismatch; tiles through `TilePlane::same_bits`, two buffers
    /// by `Arc::ptr_eq` or per-sample bits, a mixed pair through the tiled
    /// side's `equals_contiguous` — all `Sample::same_bits` (`-0.0` differs
    /// from `0.0`, a NaN pattern equals itself).
    pub fn same_samples(&self, other: &Self) -> bool {
        if self.dimensions() != other.dimensions() {
            return false;
        }
        match (&self.pixels, &other.pixels) {
            (Pixels::U16(x), Pixels::U16(y)) => x.same(y),
            (Pixels::F32(x), Pixels::F32(y)) => x.same(y),
            _ => false,
        }
    }
    /// Pixel payload bytes, not process RSS: `w·h·bpp` for a tiled plane,
    /// the allocation's capacity for a contiguous one.
    pub fn storage_bytes(&self) -> u64 {
        match &self.pixels {
            Pixels::U16(p) => p.storage_bytes(self.width, self.height),
            Pixels::F32(p) => p.storage_bytes(self.width, self.height),
        }
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        self.record_storage_resources(inventory, false);
    }
    fn record_storage_resources(&self, inventory: &mut Inventory, cache: bool) {
        let tiled = match &self.pixels {
            Pixels::U16(p) => {
                p.record(inventory, cache);
                p.is_tiled()
            }
            Pixels::F32(p) => {
                p.record(inventory, cache);
                p.is_tiled()
            }
        };
        if tiled && cache {
            inventory.pin(Arc::new(self.clone()));
        }
        if let Some(v) = self.pinned.get() {
            if cache {
                inventory.pin(v.clone());
            }
            inventory.vec(v, "precision_views", cache);
        }
        if cache {
            inventory.pin(self.profile.clone());
        }
        inventory.object(&*self.profile, "profile", cache);
        self.profile.record_resources_with_cache(inventory, cache);
    }
    /// The address the colour conversion deduplicates shared planes by
    /// (module doc of `precision_color`): the tile index a report records as
    /// `tile_structure`, or a contiguous plane's buffer. Since 13B-3e-4a
    /// also the storage identity `dr_precision_image_storage_identity`
    /// exports.
    pub(crate) fn plane_key(&self) -> usize {
        match &self.pixels {
            Pixels::U16(p) => p.key(),
            Pixels::F32(p) => p.key(),
        }
    }
    /// A `Weak` of the identity token: the typed content-box memo's key
    /// (`precision_content_box`), `LayerRaster::key`'s twin. It pins no
    /// samples.
    pub(crate) fn identity_key(&self) -> Weak<()> {
        Arc::downgrade(&self.identity)
    }
    /// Live weak handles onto this image's identity (the typed content-box
    /// memo's keys). Test oracle only, `LayerRaster::weak_count`'s twin.
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.identity)
    }
    /// Tiles shared by pointer at the same index; 0 unless both are tiled
    /// with equal dimensions and storage. Test oracle
    /// (`LayerRaster::shared_tile_count`'s twin).
    pub fn shared_tile_count(&self, other: &Self) -> usize {
        if self.dimensions() != other.dimensions() {
            return 0;
        }
        match (&self.pixels, &other.pixels) {
            (Pixels::U16(a), Pixels::U16(b)) => a.shared_tile_count(b),
            (Pixels::F32(a), Pixels::F32(b)) => a.shared_tile_count(b),
            _ => 0,
        }
    }
    /// The bbox of the tiles not shared with `other` (`Plane::unshared_rect`;
    /// the whole of `self` on a storage or dimension mismatch). No allocation.
    pub(crate) fn unshared_rect(&self, other: &Self) -> Option<(u32, u32, u32, u32)> {
        let whole = Some((0, 0, self.width, self.height));
        match (&self.pixels, &other.pixels) {
            _ if self.dimensions() != other.dimensions() => whole,
            (Pixels::U16(a), Pixels::U16(b)) => a.unshared_rect(b, self.dimensions()),
            (Pixels::F32(a), Pixels::F32(b)) => a.unshared_rect(b, self.dimensions()),
            _ => whole,
        }
    }
    pub(crate) fn pixel(&self, index: usize) -> [f32; 4] {
        match &self.pixels {
            Pixels::U16(p) => plane::normalized16(p.sample(index, self.width)),
            Pixels::F32(p) => p.sample(index, self.width),
        }
    }
    /// Row `y`, columns `x0..x1` (exclusive), as borrowed runs `(x, run)`:
    /// `TilePlane::row_segments`'s rules verbatim (one run per tile column,
    /// left to right, adjoining, `x` = the run's first column; empty for
    /// `y >= height` or `x0 >= x1`; `x1` clamped to the width). A contiguous
    /// plane answers ONE run. No allocation, no view.
    pub fn row_segments(&self, y: u32, x0: u32, x1: u32) -> PrecisionRowSegments<'_> {
        let dims = self.dimensions();
        let inner = match &self.pixels {
            Pixels::U16(p) => Rows::U16(p.row_segments(dims, y, x0, x1)),
            Pixels::F32(p) => Rows::F32(p.row_segments(dims, y, x0, x1)),
        };
        PrecisionRowSegments { inner }
    }
    /// The sample at `(x, y)` normalized as [`Self::pixel`] (u16 through the
    /// one `/ 65535` rule, f32 verbatim); `None` outside the image. No
    /// allocation, no view.
    pub fn sample_at(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        let dims = self.dimensions();
        match &self.pixels {
            Pixels::U16(p) => p.sample_at(dims, x, y).map(plane::normalized16),
            Pixels::F32(p) => p.sample_at(dims, x, y),
        }
    }
    /// The STORED u16 code at `(x, y)` (no normalization); `None` for f32
    /// storage or outside the image. No allocation, no view.
    pub fn rgba16_at(&self, x: u32, y: u32) -> Option<[u16; 4]> {
        let Pixels::U16(p) = &self.pixels else {
            return None;
        };
        p.sample_at(self.dimensions(), x, y)
    }
    /// Every row's u16 runs in plane order (row-major, left to right):
    /// `row_segments(y, 0, width)` for `y` in `0..height`. `None` for f32
    /// storage. No allocation, no view.
    pub fn runs_rgba16(&self) -> Option<impl Iterator<Item = &[[u16; 4]]> + '_> {
        let Pixels::U16(p) = &self.pixels else {
            return None;
        };
        let dims = self.dimensions();
        Some((0..dims.1).flat_map(move |y| p.row_segments(dims, y, 0, dims.0).map(|(_, run)| run)))
    }
    /// The f32 twin of [`Self::runs_rgba16`]; `None` for u16 storage.
    pub fn runs_rgba32(&self) -> Option<impl Iterator<Item = &[[f32; 4]]> + '_> {
        let Pixels::F32(p) = &self.pixels else {
            return None;
        };
        let dims = self.dimensions();
        Some((0..dims.1).flat_map(move |y| p.row_segments(dims, y, 0, dims.0).map(|(_, run)| run)))
    }

    /// Explicit source conversion. Same-storage conversion shares the handle;
    /// float→integer rounds once, including alpha; latent RGB is independent.
    /// No product path calls it: tests use it as a fixture conversion. A
    /// storage change charges `2·count·bpp` beside the bookkeeping: the
    /// output `Vec` plus its tiles.
    #[doc(hidden)]
    pub fn convert(
        &self,
        storage: PrecisionStorage,
        working_limit_bytes: u64,
        other_retained_bytes: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let count = pixel_count(self.width, self.height)?;
        let scratch = BOOKKEEPING_BYTES
            + if storage == self.storage() {
                0
            } else {
                2 * count as u64 * storage.bytes_per_pixel()
            };
        admit(
            self.retained_bytes(),
            scratch,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(scratch)?;
        let out = if storage == self.storage() {
            self.clone()
        } else {
            Self::from_float_samples_unseeded(
                self.width,
                self.height,
                self.profile.clone(),
                storage,
                (0..count).map(|i| self.pixel(i)),
                job,
            )?
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Explicit lossy RGBA8 preview/export pixels. The profile remains available
    /// on this source; callers must carry it when displaying/encoding the copy.
    pub fn to_rgba8(
        &self,
        working_limit_bytes: u64,
        other_retained_bytes: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let scratch = rgba8_preview_scratch(self.width, self.height)?;
        admit(
            self.retained_bytes(),
            scratch,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(scratch)?;
        if self.storage() == PrecisionStorage::Rgba16Unorm {
            return self
                .quantize_with_context(crate::precision_quantize::PrecisionDither::None, job);
        }
        // F32 storage is EXTENDED RANGE and scene-linear (Phase 11F): a
        // straight `v · 255` would silently clip every above-white sample
        // and blacken every negative one. Display goes through the
        // document's own tone map (`PrecisionDocument::display_rgba8`).
        Err(FLOAT_RGBA8_REFUSAL.into())
    }
}
