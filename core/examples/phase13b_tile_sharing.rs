//! Reproducible Phase 13B-1 tile-store sharing measurements. Run in isolation
//! with `cargo run --release --example phase13b_tile_sharing`; output is JSON
//! lines, one per operation per corpus size (`tests/common/phase4_corpus.rs`
//! at 24 and 48 MP, RGBA8). `from_contiguous` / `to_contiguous` / `filled`
//! account one plane; `dab_sequence` accounts the base plane plus 8 states,
//! each a 300×300 rect at `(100 + 512·i, 100)` over the previous state (4
//! tiles each). `unique_*` counts allocations once (pointer identity, as the
//! resource report does), `referenced_*` counts every plane's reference,
//! `shared_bytes_saved = referenced_bytes − unique_bytes`, and
//! `contiguous_bytes` is what the same planes retain as untiled buffers
//! (planes × w·h·4). `ms` is the operation's wall time (the 8 edits together
//! for `dab_sequence`). A storage/sharing probe, not a peak-memory
//! qualification.
//!
//! Two document-level ops (13B-2b) go through the C API: `document_dab` paints
//! the same 8 opaque 300×300 dabs through `dr_doc_painting_layer` over an
//! `DrDocument` built from the corpus and reports the 9 handles'
//! `dr_mixed_resources_report_with_raw_masks` figures (`ms` is the 8 paints together);
//! `flattened` renders a 3-layer 24 MP document (base plus two canvas-sized
//! image layers) once after `dr_mixed_resources_evict_caches` (`ms_cold`) and once
//! more (`ms_warm`), and reports how many contiguous layer views the cache
//! holds afterwards (0 since 13B-2c-1: the compositor reads rows straight
//! from the tiles). `regional_window` (13B-2c-1) builds the same 3-layer
//! document, evicts, and renders the 1024×1024 window at (2500, 1500) through
//! `render_region` at divisor 1 cold and warm, reporting the views after.
//!
//! `rect_writers` (13B-2c-2a) runs five rect-bounded writers once each over
//! the 24 MP corpus base, one state per op — `dr_doc_heal_layer` (an opaque
//! 300×300 dab at (100, 100)), `dr_doc_dodge_burn_layer` (the same dab,
//! exposure 0.5, midtones, burn), `dr_doc_clear_selection` (300×300 coverage
//! 255 at (100, 100)), `dr_doc_with_layer_plane` (red, a canvas-sized plane
//! differing from the base only inside that box) and
//! `dr_doc_content_aware_fill_edit` (a 64×64 hole at (100, 100), ring 16,
//! seed 1, sample current, in place, no job control). Each op evicts the
//! views first (the base's `from_pixels` seed must not count), then reports `[base, state]`: the
//! layer-pixel allocations and bytes, and the contiguous views the op left
//! behind (`contiguous_view_bytes`, `contiguous_views.allocation_count`).
//!
//! `placed_source` (13B-2c-2c) places the 24 MP corpus as a byte source over a
//! corpus host (`dr_doc_mixed_source_edit` `place` at (0, 0) with `source16`
//! NULL, the input handle a second `from_pixels` of the corpus, evicted first
//! and dropped after the place) and reports `[host, placed]` (line
//! `placed_source/place`); then one opaque 300×300 dab at (100, 100) on the
//! placed layer through
//! `dr_doc_painting_layer`, reporting `[host, placed, dabbed]` (line
//! `placed_source/dab`). Both lines carry the layer-pixel figures,
//! `embedded_source_pixels.unique_bytes`, `tile_structure.unique_bytes`, the
//! contiguous views and `unique_retained_bytes`. Only after the dab line is
//! printed (so no save bind enters either report) is `placed` saved with
//! `dr_doc_save_native` to a temporary file (views evicted first) and hashed
//! (`rz_sha256`, `rz_bytes`) (line `placed_source/save`): the stream must be
//! identical before and after 13B-2c-2c and 13B-2c-3a. Since 13B-2c-3a the
//! save line also carries the views the save leaves on `[host, placed,
//! dabbed]` (`contiguous_view_bytes`, `contiguous_views.allocation_count`,
//! measured after the save and before the open), and the saved file is
//! reopened with `dr_doc_open` and reported over `[reopened]` (line
//! `placed_source/reopen`, `ms` the open's wall time) before it is deleted.
//!
//! `transforms` (13B-2c-3b) runs four geometry producers over the 24 MP
//! corpus base. COLD rows (`op` `transforms`): each evicts the views first,
//! runs once and reports `[base, state]` — `dr_doc_transform_layer` (10°
//! about the origin, bilinear), `dr_doc_rotate90`, `dr_doc_resize` (3000×2000,
//! Lanczos3) and `dr_doc_perspective_layer` (a mild quad, bilinear): the
//! layer-pixel bytes, the contiguous views left behind and `ms`. WARM rows
//! (`op` `transforms/warm`) read the SAME input plane twice with NO eviction
//! between the two calls of a pair (one eviction before the pair), both on
//! a HALF-size 3000×2000 corpus plane, because at 24 MP a bilinear warp
//! (`w·h·4·64` work units) exceeds `MAX_WORK_UNITS`: `DrDocument::warp_edit`
//! with a 3×3 bilinear field (the `tests/common/warp_fixture.rs` formula,
//! inlined), then a 4×4 field on the first's output (which re-reads the
//! binding's `source_pixels`, the input plane). Each call prints the views
//! held over every live handle and its `ms`: the `#2` row is the
//! repeated-edit cost of the transient input copy. (The live-filter pair
//! this procedure also ran was removed with live filters, 2026-10-10.)
//!
//! `typed` (13B-3b-1) promotes the 24 MP corpus `v·257` into a
//! `PrecisionDocument` (`PrecisionDocument::from_image`). `typed/dab` paints
//! the same 8 opaque 300×300 dabs at `(100 + 512·i, 100)` through
//! `dr_precision_doc_edit` (a `paint` request with canvas-sized u16 coverage,
//! 65535 inside the dab) and reports `[base, 8 states]` through
//! `dr_mixed_resources_report_with_raw_masks`: `precision_pixels` unique/referenced bytes,
//! `tile_structure.unique_bytes`, the contiguous views and
//! `unique_retained_bytes` (`ms` is the 8 edits together).
//! `typed/projection` evicts the views (`dr_mixed_resources_evict_caches(NULL, 0, NULL, 0)`),
//! renders the dabbed document through `dr_precision_doc_render` cold and
//! warm (`ms_cold`, `ms_warm`), reports the views held over the dabbed
//! document and both projections, and hashes the warm projection's
//! `dr_precision_image_pixels_rgba16` bytes (`sha256`). `typed/save` saves the
//! dabbed document with `dr_precision_doc_save_native` to a temporary file,
//! hashes the file (`sha256`, `rz_bytes`), reports the views held over the
//! dabbed document after the save, and deletes the file. Both `sha256` lines
//! must be identical before and after 13B-3b-1.
//!
//! `typed/promote` (13B-3b-2) evicts every view, builds the 24 MP byte corpus
//! document (`document_dab`'s builder) and promotes it through
//! `dr_doc_promote_structure`, reporting over the promoted document its `ms`,
//! the contiguous views it holds and `precision_pixels.unique_bytes`; then ONE
//! `dr_precision_doc_render` of it (`ms_first_projection`) and the views held
//! over the promoted document afterwards (the 13B-3c hand-off figure).
//!
//! 13B-3c-1 adds `contiguous_view_bytes_document` to `typed/projection` (the
//! views held over the dabbed document alone; 0 since the typed compositor
//! reads rows straight from the tiles) and two lines. `typed/regional_window`
//! evicts the views and renders the dabbed document's 1024×1024 window at
//! (2500, 1500), divisor 1, through `dr_precision_doc_render_region` cold and
//! warm, reporting `ms_cold`, `ms_warm`, `contiguous_view_bytes_document`,
//! the views over the document and both windows (`contiguous_view_bytes`)
//! and the `sha256` of the warm window's `dr_precision_image_pixels_rgba16`
//! bytes. `typed/float_projection` writes the 3000×2000 corpus at `v/255` as
//! a float TIFF (`tiff_float::encode_to_vec`), opens it with
//! `PrecisionDocument::open_preserving`, deletes the file, then reports the
//! same keys for a cold and a warm `dr_precision_doc_render`, hashing the
//! warm projection's `to_rgba32` bytes. Every `sha256`
//! must be identical before and after 13B-3c-1.
//!
//! `typed/consumers` (13B-3c-2a) measures every consumer of a typed
//! projection handle, one line each (`consumer`, `depth`). The u16 set runs
//! over ONE `dr_precision_doc_render` of the 24 MP corpus `v·257`
//! (`PrecisionDocument::from_image`): `preview_rgba8`
//! (`dr_precision_image_preview_rgba8`), `quantize_ordered`
//! (`dr_precision_image_quantize`, dither 1), `histogram`
//! (`dr_precision_image_histogram_checked`, stride 1), `sample`
//! (`dr_precision_image_sample` at (2500, 1500), reach 2), `compare`
//! (`dr_precision_image_compare` of the projection with the document's
//! raster), `proof_render` (`dr_precision_proof_render` under
//! `{"gamut_warning":true}`, FOGRA39 when installed, else sRGB) and
//! `scoped_layer_histogram` (`dr_precision_doc_scoped_analysis`, layer 0,
//! stride 1); the f32 set over `typed/float_projection`'s 3000×2000 float
//! document and one render of it: `display_rgba8`
//! (`dr_precision_doc_display_rgba8`), `float_scoped_histogram`
//! (`dr_precision_image_scoped_analysis`, composite, stride 1, no coverage)
//! and `float_sample`
//! (`dr_precision_image_float_sample` at (1500, 1000), reach 2). Per
//! consumer: `dr_mixed_resources_evict_caches(NULL, 0, NULL, 0)`, the call (`ms_cold`),
//! `contiguous_view_bytes()` (`contiguous_view_bytes_after_cold`), the call
//! again (`ms_warm`); `sha256` hashes the warm answer (RGBA8 bytes, JSON
//! text, or the proof's bytes followed by its info JSON). Every `sha256`
//! must be identical before and after 13B-3c-2a; the view figure is 0 after.
//!
//! `typed/unseeded` (13B-3c-2b) measures the consumers of the renders that
//! stop seeding the view cache, one line each (`consumer`, `depth`). Per
//! consumer: `dr_mixed_resources_evict_caches(NULL, 0, NULL, 0)`; the inputs built,
//! untimed (a host projection is a FRESH `dr_precision_doc_render` made
//! after the eviction, so a render that seeds leaves its view live); call 1
//! (`ms_first`), then `contiguous_view_bytes()`
//! (`contiguous_view_bytes_after_first`, the process-wide figure, inputs'
//! own seeds included); call 2 on the same inputs (`ms_warm`); `sha256`
//! hashes answer 2. `copy_merged`: `PrecisionDocument::clipboard_copy` of
//! the whole canvas (`copy_merged`, no coverage) over the 24 MP corpus
//! `v·257` (`from_image`), answering `ClipboardPayload::encode`.
//! `narrow_preview`: `narrow_structure_preview(Narrowed::Rgba16(..))` of
//! `typed/float_projection`'s 3000×2000 float document against its
//! `narrow_structure_to_u16` twin (built untimed), answering the JSON.
//! `promote_preview`: `dr_doc_promote_structure_preview` of the 24 MP byte
//! corpus against its `dr_doc_promote_structure` result (built untimed).
//! `export_png16`: `dr_precision_doc_export_encode(doc, projection,
//! {"format":"png","bit_depth":16})` over the 24 MP u16 document and a
//! fresh projection, answering the encoded bytes; `export_png16_half` adds
//! `"size":{"mode":"scale","value":0.5}`; `save_psd`:
//! `dr_precision_doc_save_psd(doc, projection, tmp, 0, 0, ..)`, answering the
//! written file (deleted after each call); `export_float32`: the 3000×2000
//! float document and a fresh projection with
//! `{"format":"tiff","bit_depth":32}`. Every `sha256` must be identical
//! before and after 13B-3c-2b; from 13B-3c-2b until 13B-3e-1 the
//! host-projection readers (the exports) paid one materialization of the
//! projection at call 1, which `ms_first` measures; since 13B-3e-1 they read
//! its tiles and pay none (`typed/exports`).
//!
//! `typed/plates` (13B-3c-2c-1) measures the retouch plates, the selection
//! source and the typed resample inputs, one line each (`consumer`,
//! `depth`). The u16 document is the 24 MP corpus `v·257` plus a second
//! raster (the corpus pattern at half size, opacity 0.8) at `(−w/6, 2h/3)`,
//! partly off the canvas; the float one `typed/float_projection`'s
//! 3000×2000 document. Per line: the inputs built (untimed),
//! `dr_mixed_resources_evict_caches(NULL, 0, NULL, 0)`, call 1 (`ms_first`),
//! `contiguous_view_bytes()` while call 1's answer lives (`views_held`) and
//! after it is freed (`views_after`), call 2 (`ms_warm`), whose answer
//! `sha256` hashes; a line the core refuses at 24 MP runs at 3000×2000 and
//! prints that size. `retouch_source_current` / `retouch_source_below`:
//! `dr_precision_doc_retouch_source` of layer 1, sample 0 / 1.
//! `transform_retouch_source`: `dr_precision_image_transform_retouch_source`
//! of a held Current plate (translate (0.5, −0.25)). `retouch_clone`: a
//! `retouch_clone` of layer 0 from `source_layer` 1 over the box `(w/3,
//! h/4, w/10, h/7)`, translated onto the second raster;
//! `retouch_clone_new_layer`: layer 1, `current_and_below`, `new_layer`;
//! `spot_heal`: a 64×64 bound at `(5w/12, 3h/8)` with a 32×32 hole inside.
//! `select_luminance_composite` / `select_luminance_layer` (layer 1) /
//! `select_wand` (composite, (1000, 700), tolerance 3000) /
//! `select_color_range` (composite) through `dr_precision_doc_select`.
//! `display_rgba8`: the float document's tone-mapped display of a fresh
//! render. `geometry_affine_bilinear` (layer 1), `geometry_resize_bicubic`
//! (4500×3000) and `geometry_rotate90_float` through
//! `PrecisionDocument::geometry`; `export_png16_half`: `typed/unseeded`'s
//! half-size PNG16 export of the two-raster document and a fresh render.
//! Every `sha256` must be identical before and after 13B-3c-2c-1.
//!
//! `typed/clipboard` (13B-3c-2c-2) measures the Copy / Cut payload, built
//! unseeded since 13B-3c-2c-2, and its readers, one line each with
//! `typed/plates`' fields and procedure (inputs built untimed, eviction,
//! call 1, `views_held` / `views_after`, call 2 hashed; a line refused at
//! 24 MP runs at 3000×2000). `copy_layer`: `dr_precision_doc_clipboard_copy`
//! of layer 0 of `typed/plates`' u16 document over the whole canvas, no
//! coverage, answering the handle's `dr_clipboard_payload_bytes` (the RZCB
//! stream); `copy_merged`: the same window through `copy_merged`;
//! `copy_layer_float`: layer 0 of the 3000×2000 float document;
//! `cut_layer`: `dr_precision_doc_clipboard_cut` of layer 0 over the whole
//! canvas, answering the stream and the erased document's samples;
//! `paste_convert`: an in-memory `clipboard_copy` of the corpus `v·257` as
//! a one-raster Display P3 document (built untimed) pasted with `convert`
//! into the u16 document, answering the pasted document's samples. Every
//! `sha256` must be identical before and after 13B-3c-2c-2.
//!
//! `typed/dab` (13B-3d-1) also reports `contiguous_view_bytes_base` (the
//! views over `[base]` before the first dab), `ms_dabs` (each dab's wall
//! time), `ms_median` (the mean of the middle two), `checkpoints` (each
//! dab's `DrJobControl::checkpoints()`, a fresh control per dab) and
//! `sha256` (the last state's layer-0 u16 samples, little-endian);
//! `typed/dab_blend` paints the same 8 dabs from the same base through the
//! native `paint_blend` tool (`{"op":"paint_blend","layer":0,"color":
//! [0.312501,0.500009,0.812503,1.0],"color_space":"document","opacity":1.0,
//! "blend_mode":0}`) and reports the same keys. Both print before
//! `typed/projection`, which still renders the paint chain's last state.
//! Since the typed paint writers walk only the dab's rect, `contiguous_view_bytes`
//! must equal `contiguous_view_bytes_base` and `sha256` must be identical
//! before and after 13B-3d-1.
//!
//! 13B-3d-2 adds four lines, printed at the end of the full run and alone by
//! `DR_TILE_SHARING_ONLY=retouch`, each with `typed/dab`'s keys:
//! `typed/retouch_dab` runs the same 8 dabs over a fresh `typed_base` through
//! `{"op":"retouch_clone","layer":0,"sample":"current","matrix":[1,0,0,1,0.5,
//! 1000.25],"opacity":1.0}` (the matrix maps destination centres to source
//! centres, so the dab rows 100..400 sample rows 1100..1400 — on the canvas
//! and untouched by earlier dabs); `typed/locked_dab` paints `typed/dab`'s
//! request over the base after `{"op":"layer_lock","layer":0,"locks":1}`;
//! `typed/scalar_dab` runs `{"op":"scalar_edit","target":"red","index":0,
//! "operation":{"kind":"paint","value":65535,"opacity":1.0},"selection":
//! false}` (each dab's input is the previous dab's answer). A line refused on
//! admission at 24 MP runs at 3000×2000 (its `width` / `height` say so).
//! `typed/whole_writers` (`typed/plates`' fields and procedure, 24 MP else
//! 3000×2000) times `PrecisionDocument::apply_adjustment` of a
//! `precision_v1` invert on layer 0 of a fresh `typed_base`, hashing the
//! answer's layer-0 u16 samples. Every `sha256` must be identical before and
//! after 13B-3d-2.
//!
//! 13B-3d-3a adds two procedures, printed last in the full run and alone by
//! `DR_TILE_SHARING_ONLY=producers`. `typed/producers` (`typed/plates`'
//! fields and procedure) times each typed LAYER producer that stops seeding:
//! over `typed/plates`' u16 document at 24 MP (else 3000×2000 on refusal)
//! `merge_down` (layer 1), `merge_visible`, `stamp_visible` (anchor 1) and
//! `flatten` through `dr_precision_doc_edit`; `rasterize` (a canvas-sized
//! shape at (0, 0) over canvas-sized coverage `(i·7919) mod 65536`);
//! `artwork` (`dr_artwork_render` of one red coverage over that same
//! coverage, answering the handle's samples); `bake` (`typed_base` plus a
//! top-level `precision_v1` invert through `add_adjustment`, converted to
//! Display P3 with `{"adjustments":"preserve_appearance"}`); and at 3000×2000
//! `warp` (`warp_request(4, 4)` on layer 0 of `typed/plates`' document; 24
//! MP exceeds `MAX_WORK_UNITS`), `float_to_u16` and `narrow_to_u16`
//! (`narrow_structure_to_u16`) over `float_corpus_document`. Each answer is
//! hashed as `typed/plates`' document bytes (the artwork its image bytes).
//! `typed/producer_consumers` (its own procedure, NO eviction between the
//! producer and the consumer): `typed/plates`' document built, the views
//! evicted, flattened untimed, `views_before`; the consumer timed twice
//! (`ms_first`, then `ms_warm` after the first answer is dropped),
//! `views_held` while the second answer lives, `sha256` of the second
//! answer — `warp_after_flatten` (`warp_edit` with `warp_request(4, 4)` at
//! 3000×2000) and `save_after_flatten` (`encode_native(REFERENCE_WORKING_BYTES, 0,
//! None)` at 24 MP, answering the stream). Every `sha256` must be identical
//! before and after 13B-3d-3a.
//!
//! 13B-3d-3b adds `typed/filled`, printed last in the full run and alone by
//! `DR_TILE_SHARING_ONLY=filled`, with `typed/plates`' fields and procedure
//! (inputs built untimed, eviction, call 1, `views_held` / `views_after`,
//! call 2 hashed) plus, over answer 2, `layer_retained_bytes` and
//! `layer_plane_bytes`. `add_layer` (`add_layer_at("Empty", 0, Top)`) over
//! the 24 MP corpus `v·257` (`typed_base`, u16) and over
//! `float_corpus_document` at 3000×2000 (f32): `layer_retained_bytes` is the
//! added layer's `layer_source(top).retained_bytes()` (its profile
//! included), `layer_plane_bytes` that image's `precision_pixels` +
//! `tile_structure` unique bytes in `dr_mixed_resources_report_with_raw_masks` (the plane
//! alone), and the answer is hashed as `typed/plates`' document bytes.
//! `narrow_masked_adjustment`: `typed_base` at 24 MP plus a top-level
//! `precision_v1` invert (`add_adjustment`) carrying a canvas-sized mask
//! (`set_mask`, `(i·7919) mod 65536`), narrowed through
//! `narrow_structure_to_u8` (no dither); both figures are the byte
//! document's `layer_pixels.unique_bytes` (`dr_mixed_resources_report_with_raw_masks`) and the
//! answer is hashed as every layer's offset, RGBA8 bytes and mask bytes.
//! Its `views_held` was the base raster's seed (`w·h·4`) until 13B-3e-4b
//! unseeded the narrowing's byte planes; since then it is 0.
//!
//! `DR_TILE_SHARING_ONLY=typed` runs only the typed lines (`typed/*`): the
//! per-size byte loop and the five byte document ops are skipped.
//! `DR_TILE_SHARING_ONLY=consumers` runs only `typed/consumers`,
//! `DR_TILE_SHARING_ONLY=unseeded` only `typed/unseeded`,
//! `DR_TILE_SHARING_ONLY=plates` only `typed/plates`,
//! `DR_TILE_SHARING_ONLY=clipboard` only `typed/clipboard`,
//! `DR_TILE_SHARING_ONLY=dab` only `typed/dab` and `typed/dab_blend`, and
//! `DR_TILE_SHARING_ONLY=retouch` only the four 13B-3d-2 lines, and
//! `DR_TILE_SHARING_ONLY=producers` only the two 13B-3d-3a procedures, and
//! `DR_TILE_SHARING_ONLY=filled` only `typed/filled`, and
//! `DR_TILE_SHARING_ONLY=exports` only `typed/exports`, and
//! `DR_TILE_SHARING_ONLY=whole_readers` only `typed/whole_readers`.
//!
//! 13B-3e-1 adds `typed/exports`, printed last in the full run and alone by
//! `DR_TILE_SHARING_ONLY=exports`, with `typed/plates`' fields and procedure
//! (the inputs and a fresh projection built untimed, eviction, call 1
//! `ms_first`, `views_held` / `views_after`, call 2 `ms_warm` hashed
//! `sha256`; a line refused at 24 MP runs at 3000×2000 and prints that
//! size). Over `typed/plates`' u16 document: `export_png16_sharpen`
//! (`dr_precision_doc_export_encode` of the projection, PNG16 with the
//! `{"radius":1.4,"amount":1.75,"threshold":0.0}` sharpen),
//! `export_tiff16_p3` (TIFF16, `"profile":"display-p3"`),
//! `export_png16_render` (PNG16, NULL projection: the export renders),
//! `export_layer_trimmed` (`export_layer_encoded` of layer 1, trimmed,
//! PNG16), `export16_png` (`dr_precision_doc_export16`, PNG, the file's
//! bytes), `save_psd_flattened` (`dr_precision_doc_save_psd` mode 1) and
//! `save_psd_layers_baked` (mode 0, layer 1 styled
//! `{"effects":[{"type":"stroke","size":0.5}]}` through `layer_style`, so
//! the style is baked); over `float_corpus_document` at 3000×2000:
//! `export_float32` (TIFF32 of a fresh projection), `export_float_png16`
//! (PNG16) and `save_native_float` (`encode_native`, the trailing `LIDS`
//! ids zeroed). Every `sha256` must be identical before and after 13B-3e-1
//! and every `views_held` 0 after it.
//!
//! 13B-3e-2's `typed/live_filter` line was removed with live filters
//! (2026-10-10).
//!
//! 13B-3e-3a adds `typed/whole_readers`, printed last in the full run and
//! alone by `DR_TILE_SHARING_ONLY=whole_readers`, with `typed/plates`'
//! fields and procedure (inputs built untimed, eviction, call 1 `ms_first`,
//! `views_held` / `views_after`, call 2 `ms_warm` hashed `sha256`; a line
//! refused at 24 MP runs at 3000×2000). Over `typed/plates`' u16 document:
//! `selected_pixelate` (`selected_edit` Pixelate, block 8, no coverage),
//! `selected_auto_color` (`dr_precision_doc_selected_auto_levels`,
//! whole-target Auto Color, the JSON text), `read_plane_green`
//! (`dr_precision_doc_read_plane`, layer 0 green, the u16 LE plane),
//! `refinement_cutout` (`add_refinement_cutout` of the corpus `v·257`
//! foreground under the centred-half matte), `scalar_pixelate` (a
//! `scalar_edit` Pixelate on red, block 8) and `scalar_auto_tone`
//! (`dr_precision_doc_scalar_auto_levels`, red, Auto Tone, the JSON text);
//! at 3000×2000 only (the warp's work limit): `warp_edit` (`warp_request(4,
//! 4)` on layer 0; the warm call re-reads the same source plane) and
//! `warp_save` (`encode_native` of that warp's result, the trailing `LIDS`
//! ids zeroed; the WARP original is written from the tiles). Every `sha256`
//! must be identical before and after 13B-3e-3a, every `views_held` 0
//! after it, and `ms_warm` (median of 3 samples alternated with HEAD)
//! within 1.05× HEAD's.
//!
//! 13B-3e-3b adds `typed/raw_develop`, printed last in the full run and alone
//! by `DR_TILE_SHARING_ONLY=raw_develop`, with `typed/plates`' fields and
//! procedure (inputs built untimed, eviction, call 1 `ms_first`,
//! `views_held` / `views_after`, call 2 `ms_warm` hashed `sha256`; a line
//! refused at 24 MP runs at 3000×2000). The input is ONE plain sRGB raster
//! of the corpus `v·257` (`PrecisionDocument::from_image`). Through
//! `raw_local_develop::develop`: `develop_vignette` (base only, stops −1,
//! centre (w/2, h/2), falloff 1) and `develop_local` (one variant, RGB
//! `65535 − v·257` with alpha kept, a centred-half RZLM mask, density 0.75,
//! feather 0); through `raw_source_geometry`: `geometry_exif6` and
//! `geometry_lens_bilinear` (radial [0.02, 0, 0], scales 1); through
//! `dr_precision_image_develop_preview` on the raster (width, height and
//! the RGBA8 bytes): `preview_fit` (whole, divisor 4, clipping 0),
//! `preview_fit_clipping` (divisor 4, clipping 3) and `preview_detail`
//! (1024×1024 at the centre, divisor 1, clipping 3). The three preview
//! lines report `ms_warm` as the median of 9 consecutive warm calls and
//! `ms_warm_min` their minimum. Every `sha256` must be identical before and
//! after 13B-3e-3b, and `views_held` exactly the views this run keeps: the
//! develop output's seed (`w·h·8`) on the develop lines, the geometry
//! SOURCE's view (the `raw_source_geometry.rs:392` bind, 13B-3e-4's) plus
//! the output's seed (`2·w·h·8`) on the geometry lines and the source's view
//! (`w·h·8`) on the preview lines: the preview MISSED its warm bound on tile
//! reads, so `develop_inspect.rs:136` and its feeding seed
//! `raw_source_geometry.rs:466` kept HEAD's (`designs/phase-13b-tiled-status.md`,
//! 13B-3e-3b). Since 13B-3e-4a the develop output is unseeded and the
//! geometry source is ONE transient copy, so `views_held` is 0 on the
//! develop lines, the output's seed alone (`w·h·8`) on the geometry lines
//! and unchanged (`w·h·8`) on the preview lines.
//!
//! 13B-3e-4a adds `typed/raw_intake`, printed last in the full run and alone
//! by `DR_TILE_SHARING_ONLY=raw_intake`, with `typed/plates`' fields and
//! procedure (inputs built untimed, eviction, call 1 `ms_first`,
//! `views_held` / `views_after`, call 2 `ms_warm` hashed `sha256`; a line
//! refused at 24 MP runs at 3000×2000). The untimed input is ONLY the host
//! sample buffer (the corpus `v·257`), so every call starts from host
//! samples as the app does, and the answer holds every intermediate:
//! `intake_rgba16` (`dr_precision_doc_from_rgba16`), `intake_host_pixels`
//! (`dr_precision_doc_from_host_pixels`, `rgba16_le`, orientation 1, rows
//! of `w·8` bytes), `pipeline_exif6` (the intake, then `raw_source_geometry`
//! EXIF 6), `pipeline_develop_lens` (the intake, a vignette `develop` —
//! stops −1, centre (w/2, h/2), falloff 1 — then the lens bilinear
//! geometry, radial [0.02, 0, 0], scales 1) and `pipeline_preview_fit` (the
//! intake, then `dr_precision_image_develop_preview` whole, divisor 4,
//! clipping 0). Every `sha256` must be identical before and after
//! 13B-3e-4a; `views_held` is 0 on the two intakes and one `w·h·8` (the
//! geometry output's seed, or the first preview's materialized view) on the
//! three pipelines.
//!
//! 13B-3e-4b adds `typed/decoders`, printed last in the full run and alone by
//! `DR_TILE_SHARING_ONLY=decoders`, with `typed/plates`' fields and
//! procedure (inputs built untimed, eviction, call 1 `ms_first`,
//! `views_held` / `views_after`, call 2 `ms_warm` hashed `sha256`; a line
//! refused at 24 MP runs at 3000×2000 and prints that size). The untimed
//! inputs are BYTES or FILES in the temporary directory, so every call starts
//! from what a reader reads and holds every intermediate; u16 lines run at
//! 24 MP, f32 lines at 3000×2000. `reopen_rz16_render`: `parse_native` of
//! the `.rz` of `typed_base` WARPed on layer 0 (`warp_request(4, 4)`) plus
//! an `add_layer_at` layer, then `render` (the WARP's work limit refuses 24
//! MP, so it runs at 3000×2000); `reopen_rz16_master_preview`:
//! `parse_native` of `typed_base` with a `typed_base` placed as a u16
//! retained source, `dr_precision_doc_source_master16` of the placed layer,
//! then `dr_precision_image_develop_preview` whole (divisor 4, clipping 0);
//! `reopen_rzf_render`: `float_corpus_document` plus an `add_layer_at` layer
//! through `.rz`, then `render`; `open_png16_preview`: `open_preserving` of a
//! `precision_codec::save`d PNG16 of the corpus, then the develop preview of
//! `layer_source(0)`; `open_psd16_render` / `open_psd16_composite`:
//! `dr_precision_doc_open_psd` (layered / composite) of the core's 16-bit
//! layered PSD of `typed_base` plus an `add_layer_at` layer, then `render`;
//! `open_tiff32_render`: `open_preserving` of the float corpus TIFF, then
//! `render`; `paste_rzcb16` / `paste_rzcbf`: `ClipboardPayload::decode` of
//! the RZCB stream of layer 0's whole-canvas Copy, `clipboard_paste` in place
//! into a same-size document (`typed_base` / `float_corpus_document`), then
//! `render`; `paste_rgba8`: `ClipboardPayload::from_rgba8` of the corpus
//! bytes, pasted and rendered the same way; `narrow_render`:
//! `narrow_structure_to_u8` of `typed/filled`'s masked-adjustment document,
//! then the byte `flattened`. The answer is the render, the preview or the
//! flattened RGBA8. Every `sha256` must be identical before and after
//! 13B-3e-4b, `ms_warm` (median of 3 samples alternated with HEAD) within
//! 1.05× HEAD's, and `views_held` 0 except on the two preview lines, where
//! the first whole read of the decoded master materializes one `w·h·8` view
//! (`develop_inspect.rs:136`, 13B-3e-4c's).

use std::ffi::{c_char, CStr};
use std::ptr;
use std::time::Instant;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_artwork::dr_artwork_render;
use doctored_core::ffi_channel::dr_doc_with_layer_plane;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_cut,
    DrClipboardPayload,
};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_develop_inspect::{
    dr_precision_doc_source_master16, dr_precision_image_develop_preview,
    dr_precision_image_histogram_checked,
};
use doctored_core::ffi_doc::{
    dr_doc_clear_selection, dr_doc_dodge_burn_layer, dr_doc_free, dr_doc_open,
    dr_doc_painting_layer, dr_doc_perspective_layer, dr_doc_resize, dr_doc_rotate90,
    dr_doc_save_native, dr_doc_transform_layer,
};
use doctored_core::ffi_evaluation::dr_precision_image_compare;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_float::{dr_precision_doc_display_rgba8, dr_precision_image_float_sample};
use doctored_core::ffi_heal::dr_doc_heal_layer;
use doctored_core::ffi_host_pixels::dr_precision_doc_from_host_pixels;
use doctored_core::ffi_output_color::{
    dr_precision_doc_convert_to_profile_with, dr_precision_proof_render,
};
use doctored_core::ffi_precision::dr_precision_image_preview_rgba8;
use doctored_core::ffi_precision::{
    dr_precision_doc_export16, dr_precision_doc_free, dr_precision_doc_render,
    dr_precision_doc_save_native, dr_precision_image_free, dr_precision_image_pixels_rgba16,
};
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure_preview;
use doctored_core::ffi_precision_conversion::{
    dr_precision_image_quantize, dr_precision_image_sample,
};
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_edit_plan, dr_precision_doc_read_plane,
    dr_precision_doc_scalar_auto_levels, dr_precision_doc_select,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::ffi_retained_source::{dr_doc_mixed_source_edit, dr_precision_doc_source_edit};
use doctored_core::ffi_retouch::{
    dr_precision_doc_retouch_source, dr_precision_image_transform_retouch_source,
};
use doctored_core::ffi_scoped_statistics::{
    dr_precision_doc_scoped_analysis, dr_precision_image_scoped_analysis,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_auto_levels;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{
    ClipboardRead, PastePlacement, PasteProfile, PasteRequest,
};
use doctored_core::precision_clipboard_codec::ClipboardPayload;
use doctored_core::precision_codec::{self, PrecisionFormat};
use doctored_core::precision_conversion_narrow::Narrowed;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::raw_local_develop::develop;
use doctored_core::raw_local_mask::RawLocalMask;
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::tiff_float::FloatTiff;
use doctored_core::tile_store::{accounting, TileAccounting, TilePlane};
use doctored_core::warp::WarpEdit;
use doctored_core::DrImage;
use doctored_core::RegionRequest;
use serde_json::{json, Value};

#[path = "../tests/common/phase4_corpus.rs"]
mod phase4_corpus;

#[path = "../tests/common/sha256.rs"]
mod sha256;

#[path = "../tests/common/resource_report.rs"]
mod resource_report;

/// `typed/float_projection`'s corpus size (a 16 B/px float plane).
const FLOAT_DIMENSIONS: (u32, u32) = (3000, 2000);

fn line(op: &str, (width, height): (u32, u32), planes: u64, a: TileAccounting, ms: f64) {
    let tiles = u64::from(width.div_ceil(256)) * u64::from(height.div_ceil(256));
    println!(
        "{}",
        json!({
            "op": op,
            "width": width,
            "height": height,
            "tiles": tiles,
            "unique_tiles": a.unique_tiles,
            "referenced_tiles": a.referenced_tiles,
            "unique_bytes": a.unique_bytes,
            "referenced_bytes": a.referenced_bytes,
            "shared_bytes_saved": a.referenced_bytes - a.unique_bytes,
            "index_bytes": a.index_bytes,
            "contiguous_bytes": planes * u64::from(width) * u64::from(height) * 4,
            "ms": ms,
        })
    );
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e3
}

fn report(documents: &[*const DrDocument]) -> Value {
    resource_report::report(documents, &[], &[], &[], 0, 0).expect("a resource report")
}

fn evict(documents: &[*const DrDocument]) {
    resource_report::evict(documents).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

/// Writes (or clears, `value` all zero) the 300×300 dab `i` into a
/// canvas-frame premultiplied overlay.
fn stamp(overlay: &mut [u8], width: u32, i: usize, value: [u8; 4]) {
    let x0 = 100 + 512 * i;
    for y in 100..400 {
        let at = (y * width as usize + x0) * 4;
        for pixel in overlay[at..at + 300 * 4].chunks_exact_mut(4) {
            pixel.copy_from_slice(&value);
        }
    }
}

fn document_dab((width, height): (u32, u32)) {
    let base = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        width, height,
    ))));
    let mut overlay = vec![0_u8; width as usize * height as usize * 4];
    let mut handles = vec![base];
    let mut elapsed = 0.0;
    for i in 0..8 {
        stamp(&mut overlay, width, i, [200, 40, 90, 255]);
        let start = Instant::now();
        let next = unsafe {
            dr_doc_painting_layer(handles[i], 0, overlay.as_ptr(), width, height, 0, 1.0)
        };
        elapsed += ms(start);
        assert!(!next.is_null(), "a painted document");
        handles.push(next);
        stamp(&mut overlay, width, i, [0; 4]);
    }
    let documents: Vec<*const DrDocument> = handles.iter().map(|h| *h as *const _).collect();
    let measured = report(&documents);
    println!(
        "{}",
        json!({
            "op": "document_dab",
            "width": width,
            "height": height,
            "handles": documents.len(),
            "layer_pixels.unique_bytes": category(&measured, "layer_pixels", "unique_bytes"),
            "layer_pixels.referenced_bytes": category(&measured, "layer_pixels", "referenced_bytes"),
            "layer_pixels.allocation_count": category(&measured, "layer_pixels", "allocation_count"),
            "tile_structure.unique_bytes": category(&measured, "tile_structure", "unique_bytes"),
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "unique_retained_bytes": measured["unique_retained_bytes"].as_u64().unwrap_or(0),
            "ms": elapsed,
        })
    );
    for handle in handles {
        unsafe { dr_doc_free(handle) };
    }
}

fn three_layers((width, height): (u32, u32)) -> DrDocument {
    DrDocument::from_pixels(phase4_corpus::image(width, height))
        .adding_image_layer(0, phase4_corpus::image(width, height), "Second")
        .expect("a second layer")
        .adding_image_layer(1, phase4_corpus::image(width, height), "Third")
        .expect("a third layer")
}

fn flattened((width, height): (u32, u32)) {
    let document = three_layers((width, height));
    let documents = [&document as *const DrDocument];
    evict(&documents);
    let start = Instant::now();
    let cold = document.flattened().expect("a projection");
    let ms_cold = ms(start);
    drop(cold);
    let start = Instant::now();
    let warm = document.flattened().expect("a projection");
    let ms_warm = ms(start);
    drop(warm);
    let measured = report(&documents);
    println!(
        "{}",
        json!({
            "op": "flattened",
            "width": width,
            "height": height,
            "layers": document.layers.len(),
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_views_after_warm": category(&measured, "contiguous_views", "allocation_count"),
        })
    );
}

fn regional_window((width, height): (u32, u32)) {
    let document = three_layers((width, height));
    let documents = [&document as *const DrDocument];
    let request = RegionRequest {
        x: 2500,
        y: 1500,
        width: 1024,
        height: 1024,
        preview_divisor: 1,
    };
    evict(&documents);
    let start = Instant::now();
    let cold = document
        .render_region(request, u64::MAX, None)
        .expect("a window");
    let ms_cold = ms(start);
    drop(cold);
    let start = Instant::now();
    let warm = document
        .render_region(request, u64::MAX, None)
        .expect("a window");
    let ms_warm = ms(start);
    drop(warm);
    let measured = report(&documents);
    println!(
        "{}",
        json!({
            "op": "regional_window",
            "width": width,
            "height": height,
            "layers": document.layers.len(),
            "window": [request.x, request.y, request.width, request.height],
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_views_after_warm": category(&measured, "contiguous_views", "allocation_count"),
        })
    );
}

/// A canvas-sized buffer of `channels` bytes per pixel holding `value` over
/// the `side`×`side` box at (100, 100), zero elsewhere.
fn boxed(width: u32, height: u32, channels: usize, side: usize, value: &[u8]) -> Vec<u8> {
    let mut out = vec![0_u8; width as usize * height as usize * channels];
    for y in 100..100 + side {
        let at = (y * width as usize + 100) * channels;
        for pixel in out[at..at + side * channels].chunks_exact_mut(channels) {
            pixel.copy_from_slice(value);
        }
    }
    out
}

/// One rect writer's run over the base, answering its new document.
type Writer<'a> = Box<dyn Fn() -> *mut DrDocument + 'a>;

fn rect_writers((width, height): (u32, u32)) {
    let image = phase4_corpus::image(width, height);
    // The base's red plane, inverted inside the 300×300 box (255 is odd, so
    // every inverted byte differs).
    let mut red: Vec<u8> = image.as_raw().chunks_exact(4).map(|p| p[0]).collect();
    for y in 100..400 {
        for x in 100..400 {
            let at = y * width as usize + x;
            red[at] = 255 - red[at];
        }
    }
    let base = Box::into_raw(Box::new(DrDocument::from_pixels(image)));
    let dab = boxed(width, height, 4, 300, &[200, 40, 90, 255]);
    let selection = boxed(width, height, 1, 300, &[255]);
    let hole = boxed(width, height, 1, 64, &[255]);
    let ops: [(&str, Writer); 5] = [
        (
            "heal",
            Box::new(|| unsafe {
                let mut error = ptr::null_mut();
                let out = dr_doc_heal_layer(
                    base,
                    0,
                    dab.as_ptr(),
                    width,
                    height,
                    1.0,
                    std::ptr::null(),
                    &mut error,
                );
                assert!(error.is_null(), "heal failed");
                out
            }),
        ),
        (
            "dodge_burn",
            Box::new(|| unsafe {
                dr_doc_dodge_burn_layer(base, 0, dab.as_ptr(), width, height, 0.5, 1, true, false)
            }),
        ),
        (
            "clear_selection",
            Box::new(|| unsafe {
                dr_doc_clear_selection(base, 0, selection.as_ptr(), width, height)
            }),
        ),
        (
            "with_layer_plane",
            Box::new(|| unsafe {
                dr_doc_with_layer_plane(base, 0, 0, red.as_ptr(), width, height)
            }),
        ),
        (
            "content_aware_fill",
            Box::new(|| unsafe {
                let request = std::ffi::CString::new(
                    r#"{"op":"content_aware_fill","layer":0,"x":100,"y":100,"width":64,"height":64,"ring":16,"seed":1}"#,
                )
                .unwrap();
                let mut error = ptr::null_mut();
                let out = dr_doc_content_aware_fill_edit(
                    base,
                    request.as_ptr(),
                    hole.as_ptr(),
                    ptr::null(),
                    &mut error,
                );
                assert!(error.is_null(), "content-aware fill failed");
                out
            }),
        ),
    ];
    for (op, run) in ops {
        evict(&[base as *const _]);
        let start = Instant::now();
        let state = run();
        let elapsed = ms(start);
        assert!(!state.is_null(), "{op}: an edited document");
        let measured = report(&[base as *const _, state as *const _]);
        println!(
            "{}",
            json!({
                "op": "rect_writers",
                "writer": op,
                "width": width,
                "height": height,
                "layer_pixels.allocation_count": category(&measured, "layer_pixels", "allocation_count"),
                "layer_pixels.unique_bytes": category(&measured, "layer_pixels", "unique_bytes"),
                "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
                "contiguous_views.allocation_count": category(&measured, "contiguous_views", "allocation_count"),
                "ms": elapsed,
            })
        );
        unsafe { dr_doc_free(state) };
    }
    unsafe { dr_doc_free(base) };
}

/// One `placed_source` report line.
fn placed_line(step: &str, (width, height): (u32, u32), measured: &Value, elapsed: f64) {
    println!(
        "{}",
        json!({
            "op": format!("placed_source/{step}"),
            "width": width,
            "height": height,
            "layer_pixels.unique_bytes": category(measured, "layer_pixels", "unique_bytes"),
            "layer_pixels.referenced_bytes": category(measured, "layer_pixels", "referenced_bytes"),
            "layer_pixels.allocation_count": category(measured, "layer_pixels", "allocation_count"),
            "embedded_source_pixels.unique_bytes": category(measured, "embedded_source_pixels", "unique_bytes"),
            "tile_structure.unique_bytes": category(measured, "tile_structure", "unique_bytes"),
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(measured, "contiguous_views", "allocation_count"),
            "unique_retained_bytes": measured["unique_retained_bytes"].as_u64().unwrap_or(0),
            "ms": elapsed,
        })
    );
}

fn placed_source((width, height): (u32, u32)) {
    let host = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        width, height,
    ))));
    let input = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        width, height,
    ))));
    evict(&[host as *const _, input as *const _]);
    let request = std::ffi::CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let start = Instant::now();
    let placed = unsafe {
        dr_doc_mixed_source_edit(
            host,
            input,
            ptr::null(),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let elapsed = ms(start);
    assert!(error.is_null() && !placed.is_null(), "place failed");
    unsafe { dr_doc_free(input) };
    let measured = report(&[host as *const _, placed as *const _]);
    placed_line("place", (width, height), &measured, elapsed);

    let dab = boxed(width, height, 4, 300, &[200, 40, 90, 255]);
    let start = Instant::now();
    let dabbed = unsafe { dr_doc_painting_layer(placed, 1, dab.as_ptr(), width, height, 0, 1.0) };
    let elapsed = ms(start);
    assert!(!dabbed.is_null(), "a painted document");
    drop(dab);
    let measured = report(&[host as *const _, placed as *const _, dabbed as *const _]);
    placed_line("dab", (width, height), &measured, elapsed);

    let path =
        std::env::temp_dir().join(format!("phase13b_placed_source_{}.rz", std::process::id()));
    let c_path = std::ffi::CString::new(path.to_str().expect("a UTF-8 path")).unwrap();
    let held = [host as *const _, placed as *const _, dabbed as *const _];
    evict(&held);
    let start = Instant::now();
    let saved =
        unsafe { dr_doc_save_native(placed, c_path.as_ptr(), std::ptr::null(), &mut error) };
    let elapsed = ms(start);
    assert!(saved && error.is_null(), "save failed");
    let measured = report(&held);
    let bytes = std::fs::read(&path).expect("the saved file");
    println!(
        "{}",
        json!({
            "op": "placed_source/save",
            "width": width,
            "height": height,
            "rz_sha256": sha256::sha256_hex(&bytes),
            "rz_bytes": bytes.len(),
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(&measured, "contiguous_views", "allocation_count"),
            "ms": elapsed,
        })
    );
    drop(bytes);

    let start = Instant::now();
    let reopened = unsafe { dr_doc_open(c_path.as_ptr(), std::ptr::null(), &mut error) };
    let elapsed = ms(start);
    assert!(error.is_null() && !reopened.is_null(), "open failed");
    std::fs::remove_file(&path).expect("the saved file removed");
    let measured = report(&[reopened as *const _]);
    placed_line("reopen", (width, height), &measured, elapsed);
    for handle in [reopened, dabbed, placed, host] {
        unsafe { dr_doc_free(handle) };
    }
}

/// One `transforms` cold row.
fn transform_line(writer: &str, measured: &Value, elapsed: f64) {
    println!(
        "{}",
        json!({
            "op": "transforms",
            "writer": writer,
            "layer_pixels.unique_bytes": category(measured, "layer_pixels", "unique_bytes"),
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(measured, "contiguous_views", "allocation_count"),
            "ms": elapsed,
        })
    );
}

/// One `transforms/warm` row over a `width`×`height` input plane.
fn warm_line(writer: &str, (width, height): (u32, u32), measured: &Value, elapsed: f64) {
    println!(
        "{}",
        json!({
            "op": "transforms/warm",
            "writer": writer,
            "width": width,
            "height": height,
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(measured, "contiguous_views", "allocation_count"),
            "ms": elapsed,
        })
    );
}

/// The `warp_fixture::field` request: a `columns`×`rows` bilinear lattice
/// of small non-uniform displacements on layer 0's pixels.
fn warp_request(columns: u32, rows: u32) -> WarpEdit {
    let values: Vec<[f64; 2]> = (0..rows)
        .flat_map(|y| {
            (0..columns).map(move |x| {
                let fx = x as f64 / (columns - 1) as f64;
                let fy = y as f64 / (rows - 1) as f64;
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    let request = json!({"op":"set","layer":0,"target":"pixels","columns":columns,"rows":rows,
        "displacements":values,"filter":"bilinear"});
    WarpEdit::parse(&request.to_string()).expect("a warp request")
}

fn transforms((width, height): (u32, u32)) {
    let base = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        width, height,
    ))));
    let (s, c) = 10_f64.to_radians().sin_cos();
    let affine = [c, s, -s, c, 0.0, 0.0];
    let (w, h) = (f64::from(width), f64::from(height));
    let quad = [0.0, 0.0, w, 60.0, w - 60.0, h, 30.0, h - 30.0];
    let ops: [(&str, Writer); 4] = [
        (
            "transform_layer",
            Box::new(|| unsafe { dr_doc_transform_layer(base, 0, affine.as_ptr(), 1) }),
        ),
        ("rotate90", Box::new(|| unsafe { dr_doc_rotate90(base) })),
        (
            "resize",
            Box::new(|| unsafe { dr_doc_resize(base, width / 2, height / 2, 3) }),
        ),
        (
            "perspective_layer",
            Box::new(|| unsafe { dr_doc_perspective_layer(base, 0, quad.as_ptr(), 1) }),
        ),
    ];
    for (writer, run) in ops {
        evict(&[base as *const _]);
        let start = Instant::now();
        let state = run();
        let elapsed = ms(start);
        assert!(!state.is_null(), "{writer}: a transformed document");
        let measured = report(&[base as *const _, state as *const _]);
        transform_line(writer, &measured, elapsed);
        unsafe { dr_doc_free(state) };
    }

    unsafe { dr_doc_free(base) };

    // (w1) Two warp edits on the same input plane, no eviction between (a
    // half-size base: a 24 MP bilinear warp exceeds the work-unit limit).
    let half = (width / 2, height / 2);
    let small = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        half.0, half.1,
    ))));
    evict(&[small as *const _]);
    let start = Instant::now();
    let first = unsafe { &*small }
        .warp_edit(&warp_request(3, 3), REFERENCE_WORKING_BYTES, 0, None)
        .expect("a warp")
        .expect("a warped document");
    let elapsed = ms(start);
    let first = Box::into_raw(Box::new(first));
    let measured = report(&[small as *const _, first as *const _]);
    warm_line("warp_edit#1", half, &measured, elapsed);
    let start = Instant::now();
    let second = unsafe { &*first }
        .warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)
        .expect("a warp")
        .expect("a warped document");
    let elapsed = ms(start);
    let second = Box::into_raw(Box::new(second));
    let measured = report(&[small as *const _, first as *const _, second as *const _]);
    warm_line("warp_edit#2", half, &measured, elapsed);
    for handle in [second, first, small] {
        unsafe { dr_doc_free(handle) };
    }
}

fn typed_report(documents: &[*const PrecisionDocument], images: &[*const PrecisionImage]) -> Value {
    resource_report::report(&[], &[], documents, images, 0, 0).expect("a resource report")
}

fn typed_error(error: *mut c_char, what: &str) {
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{what} failed: {message}");
    }
}

/// Writes (or clears, `value` 0) the u16 coverage of the 300×300 dab `i`.
fn stamp16(coverage: &mut [u16], width: u32, i: usize, value: u16) {
    let x0 = 100 + 512 * i;
    for y in 100..400 {
        let at = y * width as usize + x0;
        coverage[at..at + 300].fill(value);
    }
}

/// The 24 MP corpus `v·257` as a one-raster typed document.
fn typed_base((width, height): (u32, u32)) -> *mut PrecisionDocument {
    let samples: Vec<[u16; 4]> = phase4_corpus::image(width, height)
        .into_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect();
    let image = PrecisionImage::from_rgba16(width, height, samples, IccProfile::srgb(), None)
        .expect("a typed corpus image");
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .expect("a typed corpus document"),
    ))
}

/// Eight opaque 300×300 dabs of `request` from `base`, one state each (a
/// fresh `DrJobControl` per dab), printed as line `op`; answers
/// `[base, 8 states]`.
fn typed_dabs(
    op: &str,
    base: *mut PrecisionDocument,
    request: &Value,
    size: (u32, u32),
) -> Vec<*mut PrecisionDocument> {
    try_typed_dabs(op, base, request, size).unwrap_or_else(|e| panic!("typed dab failed: {e}"))
}

/// [`typed_dabs`], answering a dab's refusal (the states made so far freed,
/// nothing printed) instead of panicking.
fn try_typed_dabs(
    op: &str,
    base: *mut PrecisionDocument,
    request: &Value,
    (width, height): (u32, u32),
) -> Result<Vec<*mut PrecisionDocument>, String> {
    let request = std::ffi::CString::new(request.to_string()).unwrap();
    let base_views = typed_report(&[base as *const _], &[])["contiguous_view_bytes"]
        .as_u64()
        .unwrap_or(0);
    let mut coverage = vec![0_u16; width as usize * height as usize];
    let mut handles = vec![base];
    let mut ms_dabs = Vec::new();
    let mut checkpoints = Vec::new();
    for i in 0..8 {
        stamp16(&mut coverage, width, i, 65535);
        let control = DrJobControl::default();
        let mut error = ptr::null_mut();
        let start = Instant::now();
        let next = unsafe {
            dr_precision_doc_edit(
                handles[i],
                request.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                &control,
                &mut error,
            )
        };
        ms_dabs.push(ms(start));
        if let Err(e) = plates_error(false, error) {
            for handle in handles.into_iter().skip(1) {
                unsafe { dr_precision_doc_free(handle) };
            }
            return Err(e);
        }
        assert!(!next.is_null(), "a painted typed document");
        checkpoints.push(control.checkpoints());
        handles.push(next);
        stamp16(&mut coverage, width, i, 0);
    }
    drop(coverage);
    let documents: Vec<*const PrecisionDocument> = handles.iter().map(|h| *h as *const _).collect();
    let measured = typed_report(&documents, &[]);
    let mut sorted = ms_dabs.clone();
    sorted.sort_by(f64::total_cmp);
    let last = unsafe { &*handles[8] };
    let samples = last
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
        .collect();
    drop(samples);
    println!(
        "{}",
        json!({
            "op": op,
            "width": width,
            "height": height,
            "handles": documents.len(),
            "precision_pixels.unique_bytes": category(&measured, "precision_pixels", "unique_bytes"),
            "precision_pixels.referenced_bytes": category(&measured, "precision_pixels", "referenced_bytes"),
            "tile_structure.unique_bytes": category(&measured, "tile_structure", "unique_bytes"),
            "contiguous_view_bytes_base": base_views,
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(&measured, "contiguous_views", "allocation_count"),
            "unique_retained_bytes": measured["unique_retained_bytes"].as_u64().unwrap_or(0),
            "ms": ms_dabs.iter().sum::<f64>(),
            "ms_dabs": ms_dabs,
            "ms_median": (sorted[3] + sorted[4]) / 2.0,
            "checkpoints": checkpoints,
            "sha256": sha256::sha256_hex(&bytes),
        })
    );
    Ok(handles)
}

/// `typed/dab_blend`'s request: `typed/dab`'s colour through the native
/// `paint_blend` tool (Normal).
fn blend_request() -> Value {
    json!({"op":"paint_blend","layer":0,"color":[0.312501,0.500009,0.812503,1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0})
}

/// `typed/dab` then `typed/dab_blend` from the same base; answers the paint
/// chain `[base, 8 states]` (the blend chain's states are freed).
fn typed_dab_lines(size: (u32, u32)) -> Vec<*mut PrecisionDocument> {
    let base = typed_base(size);
    let request = json!({"op":"paint","layer":0,"color":[0.312501, 0.500009, 0.812503, 1.0],
        "color_space":"document","opacity":1.0});
    let handles = typed_dabs("typed/dab", base, &request, size);
    let blended = typed_dabs("typed/dab_blend", base, &blend_request(), size);
    for handle in blended.into_iter().skip(1).rev() {
        unsafe { dr_precision_doc_free(handle) };
    }
    handles
}

/// One 13B-3d-2 dab line: `typed_dabs` of `request` over `typed_base` (made
/// `locked` first through `layer_lock` 1 when asked) at 24 MP, else — a
/// refusal on admission — at 3000×2000; every handle freed.
fn retouch_dab_line(op: &str, request: &Value, locked: bool) {
    for size in [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS] {
        let mut base = typed_base(size);
        if locked {
            let lock =
                std::ffi::CString::new(json!({"op":"layer_lock","layer":0,"locks":1}).to_string())
                    .unwrap();
            let next = plates_edit(unsafe { &*base }, &lock, &[]).expect("a locked base");
            unsafe { dr_precision_doc_free(base) };
            base = Box::into_raw(Box::new(next));
        }
        match try_typed_dabs(op, base, request, size) {
            Ok(handles) => {
                for handle in handles.into_iter().rev() {
                    unsafe { dr_precision_doc_free(handle) };
                }
                return;
            }
            Err(e) if size != FLOAT_DIMENSIONS => {
                eprintln!("{op}: refused at {size:?} ({e}); 3000×2000");
                unsafe { dr_precision_doc_free(base) };
            }
            Err(e) => panic!("{op} failed: {e}"),
        }
    }
}

/// The 13B-3d-2 lines (`DR_TILE_SHARING_ONLY=retouch`): `typed/retouch_dab`,
/// `typed/locked_dab`, `typed/scalar_dab` and `typed/whole_writers`.
fn typed_retouch() {
    let clone = json!({"op":"retouch_clone","layer":0,"sample":"current",
        "matrix":[1,0,0,1,0.5,1000.25],"opacity":1.0});
    retouch_dab_line("typed/retouch_dab", &clone, false);
    let paint = json!({"op":"paint","layer":0,"color":[0.312501, 0.500009, 0.812503, 1.0],
        "color_space":"document","opacity":1.0});
    retouch_dab_line("typed/locked_dab", &paint, true);
    let scalar = json!({"op":"scalar_edit","target":"red","index":0,
        "operation":{"kind":"paint","value":65535,"opacity":1.0},"selection":false});
    retouch_dab_line("typed/scalar_dab", &scalar, false);
    let invert =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    timed_line(
        "typed/whole_writers",
        "apply_adjustment",
        "u16",
        &[phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS],
        |size| *unsafe { Box::from_raw(typed_base(size)) },
        |doc: &PrecisionDocument| {
            doc.apply_adjustment(0, &invert, REFERENCE_WORKING_BYTES, 0, None)
        },
        |answer: &Option<PrecisionDocument>| -> Vec<u8> {
            let doc = answer.as_ref().expect("an adjusted document");
            let samples = doc.layer(0).unwrap().source().unwrap().to_rgba16().unwrap();
            samples
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
                .collect()
        },
    );
}

fn typed((width, height): (u32, u32)) {
    // (1) Eight opaque dabs, one state each (and the `paint_blend` twin).
    let handles = typed_dab_lines((width, height));
    let dabbed = handles[8];

    // (2) The projection, cold then warm.
    resource_report::evict(&[]).expect("an eviction report");
    let render = || {
        let mut error = ptr::null_mut();
        let start = Instant::now();
        let out = unsafe {
            dr_precision_doc_render(dabbed, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
        };
        let elapsed = ms(start);
        typed_error(error, "typed render");
        assert!(!out.is_null(), "a typed projection");
        (out, elapsed)
    };
    let (cold, ms_cold) = render();
    let (warm, ms_warm) = render();
    let measured = typed_report(&[dabbed as *const _], &[cold as *const _, warm as *const _]);
    let document_views = typed_report(&[dabbed as *const _], &[])["contiguous_view_bytes"]
        .as_u64()
        .unwrap_or(0);
    let lent = unsafe { dr_precision_image_pixels_rgba16(warm) };
    assert!(!lent.is_null(), "u16 projection samples");
    let bytes = unsafe {
        std::slice::from_raw_parts(lent.cast::<u8>(), width as usize * height as usize * 8)
    };
    println!(
        "{}",
        json!({
            "op": "typed/projection",
            "width": width,
            "height": height,
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_view_bytes_document": document_views,
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(&measured, "contiguous_views", "allocation_count"),
            "sha256": sha256::sha256_hex(bytes),
        })
    );
    for image in [cold, warm] {
        unsafe { dr_precision_image_free(image) };
    }

    // (3) The native save of the dabbed document.
    let path = std::env::temp_dir().join(format!("phase13b_typed_{}.rz", std::process::id()));
    let c_path = std::ffi::CString::new(path.to_str().expect("a UTF-8 path")).unwrap();
    let mut error = ptr::null_mut();
    let start = Instant::now();
    let saved = unsafe {
        dr_precision_doc_save_native(
            dabbed,
            c_path.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let elapsed = ms(start);
    typed_error(error, "typed save");
    assert!(saved, "typed save failed");
    let measured = typed_report(&[dabbed as *const _], &[]);
    let bytes = std::fs::read(&path).expect("the saved file");
    std::fs::remove_file(&path).expect("the saved file removed");
    println!(
        "{}",
        json!({
            "op": "typed/save",
            "width": width,
            "height": height,
            "sha256": sha256::sha256_hex(&bytes),
            "rz_bytes": bytes.len(),
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "contiguous_views.allocation_count": category(&measured, "contiguous_views", "allocation_count"),
            "ms": elapsed,
        })
    );
    typed_regional_window(dabbed, (width, height));
    for handle in handles.into_iter().rev() {
        unsafe { dr_precision_doc_free(handle) };
    }
}

/// `typed/regional_window` (13B-3c-1): the dabbed document's 1024×1024
/// window at (2500, 1500), divisor 1, cold after an eviction then warm.
fn typed_regional_window(dabbed: *mut PrecisionDocument, (width, height): (u32, u32)) {
    let (x, y, w, h) = (2500, 1500, 1024, 1024);
    resource_report::evict(&[]).expect("an eviction report");
    let render = || {
        let mut error = ptr::null_mut();
        let start = Instant::now();
        let out = unsafe {
            dr_precision_doc_render_region(
                dabbed,
                x,
                y,
                w,
                h,
                1,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        let elapsed = ms(start);
        typed_error(error, "typed regional window");
        assert!(!out.is_null(), "a typed window");
        (out, elapsed)
    };
    let (cold, ms_cold) = render();
    let (warm, ms_warm) = render();
    let measured = typed_report(&[dabbed as *const _], &[cold as *const _, warm as *const _]);
    let document_views = typed_report(&[dabbed as *const _], &[])["contiguous_view_bytes"]
        .as_u64()
        .unwrap_or(0);
    let lent = unsafe { dr_precision_image_pixels_rgba16(warm) };
    assert!(!lent.is_null(), "u16 window samples");
    let bytes =
        unsafe { std::slice::from_raw_parts(lent.cast::<u8>(), w as usize * h as usize * 8) };
    println!(
        "{}",
        json!({
            "op": "typed/regional_window",
            "width": width,
            "height": height,
            "window": [x, y, w, h],
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_view_bytes_document": document_views,
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "sha256": sha256::sha256_hex(bytes),
        })
    );
    for image in [cold, warm] {
        unsafe { dr_precision_image_free(image) };
    }
}

/// `typed/float_projection` (13B-3c-1): the corpus at `v/255` as a float
/// TIFF, opened with `open_preserving` (the file deleted), rendered cold
/// after an eviction then warm.
fn typed_float_projection((width, height): (u32, u32)) {
    let samples: Vec<f32> = phase4_corpus::image(width, height)
        .into_raw()
        .into_iter()
        .map(|v| f32::from(v) / 255.0)
        .collect();
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let encoded = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("a float TIFF");
    drop(tiff);
    let path = std::env::temp_dir().join(format!("phase13b_float_{}.tiff", std::process::id()));
    std::fs::write(&path, &encoded).expect("the float TIFF written");
    drop(encoded);
    let document = PrecisionDocument::open_preserving(
        path.to_str().expect("a UTF-8 path"),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .expect("a float document");
    std::fs::remove_file(&path).expect("the float TIFF removed");
    assert!(document.is_float(), "a float document");
    let doc = Box::into_raw(Box::new(document));

    resource_report::evict(&[]).expect("an eviction report");
    let render = || {
        let mut error = ptr::null_mut();
        let start = Instant::now();
        let out = unsafe {
            dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
        };
        let elapsed = ms(start);
        typed_error(error, "float render");
        assert!(!out.is_null(), "a float projection");
        (out, elapsed)
    };
    let (cold, ms_cold) = render();
    let (warm, ms_warm) = render();
    let measured = typed_report(&[doc as *const _], &[cold as *const _, warm as *const _]);
    let document_views = typed_report(&[doc as *const _], &[])["contiguous_view_bytes"]
        .as_u64()
        .unwrap_or(0);
    let samples = unsafe { &*warm }
        .to_rgba32()
        .expect("f32 projection samples");
    let bytes: Vec<u8> = samples
        .as_flattened()
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    println!(
        "{}",
        json!({
            "op": "typed/float_projection",
            "width": width,
            "height": height,
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_view_bytes_document": document_views,
            "contiguous_view_bytes": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
            "sha256": sha256::sha256_hex(&bytes),
        })
    );
    for image in [cold, warm] {
        unsafe { dr_precision_image_free(image) };
    }
    unsafe { dr_precision_doc_free(doc) };
}

fn typed_promote((width, height): (u32, u32)) {
    resource_report::evict(&[]).expect("an eviction report");
    let byte = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
        width, height,
    ))));
    let mut error = ptr::null_mut();
    let start = Instant::now();
    let promoted = unsafe {
        dr_doc_promote_structure(byte, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    let elapsed = ms(start);
    typed_error(error, "typed promote");
    assert!(!promoted.is_null(), "a promoted document");
    unsafe { dr_doc_free(byte) };
    let measured = typed_report(&[promoted as *const _], &[]);
    let (promote_views, promote_view_count, promote_pixels) = (
        measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
        category(&measured, "contiguous_views", "allocation_count"),
        category(&measured, "precision_pixels", "unique_bytes"),
    );
    let mut error = ptr::null_mut();
    let start = Instant::now();
    let projection = unsafe {
        dr_precision_doc_render(
            promoted,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let ms_first_projection = ms(start);
    typed_error(error, "typed render");
    assert!(!projection.is_null(), "a typed projection");
    unsafe { dr_precision_image_free(projection) };
    let measured = typed_report(&[promoted as *const _], &[]);
    println!(
        "{}",
        json!({
            "op": "typed/promote",
            "width": width,
            "height": height,
            "ms": elapsed,
            "contiguous_view_bytes": promote_views,
            "contiguous_views.allocation_count": promote_view_count,
            "precision_pixels.unique_bytes": promote_pixels,
            "ms_first_projection": ms_first_projection,
            "contiguous_view_bytes_after_projection": measured["contiguous_view_bytes"].as_u64().unwrap_or(0),
        })
    );
    unsafe { dr_precision_doc_free(promoted) };
}

/// The FOGRA39 proof profile when installed, else sRGB
/// (`soft_proof_tests`' path).
const FOGRA: &str =
    "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc";

/// An owned RGBA8 answer's bytes; the handle is freed.
fn rgba8_answer(image: *mut DrImage, error: *mut c_char, what: &str) -> Vec<u8> {
    typed_error(error, what);
    assert!(!image.is_null(), "{what}: an image");
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(image) };
    bytes
}

/// An owned JSON answer's text; the string is freed.
fn text_answer(result: *mut c_char, error: *mut c_char, what: &str) -> Vec<u8> {
    typed_error(error, what);
    assert!(!result.is_null(), "{what}: a string");
    let bytes = unsafe { CStr::from_ptr(result) }.to_bytes().to_vec();
    unsafe { dr_string_free(result) };
    bytes
}

/// One `typed/consumers` line: evict, the cold call, the views left, the
/// warm call, the warm answer's hash.
fn consumer(name: &str, depth: &str, (width, height): (u32, u32), call: &dyn Fn() -> Vec<u8>) {
    resource_report::evict(&[]).expect("an eviction report");
    let start = Instant::now();
    drop(call());
    let ms_cold = ms(start);
    let views = contiguous_view_bytes();
    let start = Instant::now();
    let answer = call();
    let ms_warm = ms(start);
    println!(
        "{}",
        json!({
            "op": "typed/consumers",
            "consumer": name,
            "depth": depth,
            "width": width,
            "height": height,
            "ms_cold": ms_cold,
            "ms_warm": ms_warm,
            "contiguous_view_bytes_after_cold": views,
            "sha256": sha256::sha256_hex(&answer),
        })
    );
}

/// `typed/float_projection`'s float document (the corpus at `v/255` as a
/// float TIFF opened with `open_preserving`, the file deleted), boxed.
fn float_corpus_document((width, height): (u32, u32)) -> *mut PrecisionDocument {
    let samples: Vec<f32> = phase4_corpus::image(width, height)
        .into_raw()
        .into_iter()
        .map(|v| f32::from(v) / 255.0)
        .collect();
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let encoded = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("a float TIFF");
    drop(tiff);
    let path = std::env::temp_dir().join(format!("phase13b_consumers_{}.tiff", std::process::id()));
    std::fs::write(&path, &encoded).expect("the float TIFF written");
    drop(encoded);
    let document = PrecisionDocument::open_preserving(
        path.to_str().expect("a UTF-8 path"),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .expect("a float document");
    std::fs::remove_file(&path).expect("the float TIFF removed");
    assert!(document.is_float(), "a float document");
    Box::into_raw(Box::new(document))
}

fn typed_consumers() {
    let size = phase4_corpus::DIMENSIONS[0];
    let (width, height) = size;
    let samples: Vec<[u16; 4]> = phase4_corpus::image(width, height)
        .into_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect();
    let image = PrecisionImage::from_rgba16(width, height, samples, IccProfile::srgb(), None)
        .expect("a typed corpus image");
    let doc = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .expect("a typed corpus document"),
    ));
    let mut error = ptr::null_mut();
    let projection = unsafe {
        dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    typed_error(error, "typed render");
    assert!(!projection.is_null(), "a typed projection");
    let raster: *const PrecisionImage = unsafe { &*doc }
        .layer(0)
        .and_then(|layer| layer.source())
        .expect("the corpus raster");
    let proof_icc = std::fs::read(FOGRA).unwrap_or_else(|_| IccProfile::srgb().bytes().to_vec());
    let setup = std::ffi::CString::new(json!({"gamut_warning":true}).to_string()).unwrap();
    let scoped = std::ffi::CString::new(
        json!({"operation":"histogram","source":"layer","layer":0,"stride":1}).to_string(),
    )
    .unwrap();

    consumer("preview_rgba8", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_preview_rgba8(
                projection,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        rgba8_answer(out, error, "preview")
    });
    consumer("quantize_ordered", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_quantize(
                projection,
                1,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        rgba8_answer(out, error, "quantize")
    });
    consumer("histogram", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_histogram_checked(
                projection,
                1,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        text_answer(out, error, "histogram")
    });
    consumer("sample", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe { dr_precision_image_sample(projection, 2500, 1500, 2, &mut error) };
        text_answer(out, error, "sample")
    });
    consumer("compare", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe { dr_precision_image_compare(projection, raster, &mut error) };
        text_answer(out, error, "compare")
    });
    consumer("proof_render", "u16", size, &|| {
        let (mut info, mut error) = (ptr::null_mut(), ptr::null_mut());
        let out = unsafe {
            dr_precision_proof_render(
                projection,
                proof_icc.as_ptr(),
                proof_icc.len(),
                setup.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut info,
                &mut error,
            )
        };
        let mut answer = rgba8_answer(out, error, "proof");
        answer.extend(text_answer(info, ptr::null_mut(), "proof info"));
        answer
    });
    consumer("scoped_layer_histogram", "u16", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_scoped_analysis(
                doc,
                scoped.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        text_answer(out, error, "scoped analysis")
    });
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(doc) };

    let size = FLOAT_DIMENSIONS;
    let (width, height) = size;
    let float = float_corpus_document(size);
    let mut error = ptr::null_mut();
    let projection = unsafe {
        dr_precision_doc_render(float, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    typed_error(error, "float render");
    assert!(!projection.is_null(), "a float projection");
    consumer("display_rgba8", "f32", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_display_rgba8(
                float,
                projection,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        rgba8_answer(out, error, "display")
    });
    let composite = std::ffi::CString::new(
        json!({"operation":"histogram","source":"composite","stride":1}).to_string(),
    )
    .unwrap();
    consumer("float_scoped_histogram", "f32", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_scoped_analysis(
                projection,
                composite.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        text_answer(out, error, "float scoped histogram")
    });
    consumer("float_sample", "f32", size, &|| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_float_sample(
                projection,
                width as i32 / 2,
                height as i32 / 2,
                2,
                &mut error,
            )
        };
        text_answer(out, error, "float sample")
    });
    unsafe { dr_precision_image_free(projection) };
    unsafe { dr_precision_doc_free(float) };
}

/// One `typed/unseeded` line: evict, build the inputs (untimed), call 1,
/// the views left, call 2, the second answer's hash. Each call times
/// itself and answers `(ms, answer)`.
fn unseeded<T>(
    name: &str,
    depth: &str,
    (width, height): (u32, u32),
    build: impl FnOnce() -> T,
    call: impl Fn(&T) -> (f64, Vec<u8>),
    free: impl FnOnce(T),
) {
    resource_report::evict(&[]).expect("an eviction report");
    let inputs = build();
    let (ms_first, first) = call(&inputs);
    drop(first);
    let views = contiguous_view_bytes();
    let (ms_warm, answer) = call(&inputs);
    free(inputs);
    println!(
        "{}",
        json!({
            "op": "typed/unseeded",
            "consumer": name,
            "depth": depth,
            "width": width,
            "height": height,
            "ms_first": ms_first,
            "ms_warm": ms_warm,
            "contiguous_view_bytes_after_first": views,
            "sha256": sha256::sha256_hex(&answer),
        })
    );
}

/// The 24 MP corpus `v·257` as a one-raster u16 document, boxed.
fn u16_corpus_document((width, height): (u32, u32)) -> *mut PrecisionDocument {
    let samples: Vec<[u16; 4]> = phase4_corpus::image(width, height)
        .into_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect();
    let image = PrecisionImage::from_rgba16(width, height, samples, IccProfile::srgb(), None)
        .expect("a typed corpus image");
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .expect("a typed corpus document"),
    ))
}

/// A fresh host projection of `doc` (`dr_precision_doc_render`).
fn host_projection(doc: *const PrecisionDocument) -> *mut PrecisionImage {
    let mut error = ptr::null_mut();
    let projection = unsafe {
        dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    typed_error(error, "typed render");
    assert!(!projection.is_null(), "a typed projection");
    projection
}

/// `dr_precision_doc_export_encode` of `projection` under `request`, timed,
/// answering the encoded bytes.
fn export_encode(
    doc: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request: &std::ffi::CStr,
) -> (f64, Vec<u8>) {
    let mut error = ptr::null_mut();
    let start = Instant::now();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            projection,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let elapsed = ms(start);
    typed_error(error, "typed export");
    assert!(!result.is_null(), "an export result");
    let mut length = 0_usize;
    let bytes = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!bytes.is_null(), "export bytes");
    let answer = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    (elapsed, answer)
}

fn typed_unseeded() {
    let size = phase4_corpus::DIMENSIONS[0];
    let (width, height) = size;

    unseeded(
        "copy_merged",
        "u16",
        size,
        || {
            let doc = u16_corpus_document(size);
            let read: ClipboardRead = serde_json::from_value(
                json!({"op":"copy_merged","x":0,"y":0,"width":width,"height":height}),
            )
            .expect("a copy_merged request");
            (doc, read)
        },
        |(doc, read)| {
            let start = Instant::now();
            let payload = unsafe { &**doc }
                .clipboard_copy(read, None, REFERENCE_WORKING_BYTES, 0, None)
                .expect("a Copy Merged payload");
            let elapsed = ms(start);
            (elapsed, payload.encode().expect("an encoded payload"))
        },
        |(doc, _)| unsafe { dr_precision_doc_free(doc) },
    );

    unseeded(
        "narrow_preview",
        "f32",
        FLOAT_DIMENSIONS,
        || {
            let float = float_corpus_document(FLOAT_DIMENSIONS);
            let narrowed = unsafe { &*float }
                .narrow_structure_to_u16(REFERENCE_WORKING_BYTES, 0, None)
                .expect("the 16-bit twin");
            (float, narrowed)
        },
        |(float, narrowed)| {
            let start = Instant::now();
            let value = unsafe { &**float }
                .narrow_structure_preview(
                    Narrowed::Rgba16(narrowed),
                    REFERENCE_WORKING_BYTES,
                    0,
                    None,
                )
                .expect("a narrow preview");
            let elapsed = ms(start);
            (elapsed, value.to_string().into_bytes())
        },
        |(float, _)| unsafe { dr_precision_doc_free(float) },
    );

    unseeded(
        "promote_preview",
        "u8",
        size,
        || {
            let byte = Box::into_raw(Box::new(DrDocument::from_pixels(phase4_corpus::image(
                width, height,
            ))));
            let mut error = ptr::null_mut();
            let promoted = unsafe {
                dr_doc_promote_structure(byte, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
            };
            typed_error(error, "promotion");
            assert!(!promoted.is_null(), "a promoted document");
            (byte, promoted)
        },
        |&(byte, promoted)| {
            let mut error = ptr::null_mut();
            let start = Instant::now();
            let out = unsafe {
                dr_doc_promote_structure_preview(
                    byte,
                    promoted,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            let elapsed = ms(start);
            (elapsed, text_answer(out, error, "promote preview"))
        },
        |(byte, promoted)| unsafe {
            dr_precision_doc_free(promoted);
            dr_doc_free(byte);
        },
    );

    for (name, request) in [
        ("export_png16", json!({"format":"png","bit_depth":16})),
        (
            "export_png16_half",
            json!({"format":"png","bit_depth":16,"size":{"mode":"scale","value":0.5}}),
        ),
    ] {
        let request = std::ffi::CString::new(request.to_string()).unwrap();
        unseeded(
            name,
            "u16",
            size,
            || {
                let doc = u16_corpus_document(size);
                (doc, host_projection(doc))
            },
            |&(doc, projection)| export_encode(doc, projection, &request),
            |(doc, projection)| unsafe {
                dr_precision_image_free(projection);
                dr_precision_doc_free(doc);
            },
        );
    }

    let path = std::env::temp_dir().join(format!("phase13b_unseeded_{}.psd", std::process::id()));
    let c_path = std::ffi::CString::new(path.to_str().expect("a UTF-8 path")).unwrap();
    unseeded(
        "save_psd",
        "u16",
        size,
        || {
            let doc = u16_corpus_document(size);
            (doc, host_projection(doc))
        },
        |&(doc, projection)| {
            let mut error = ptr::null_mut();
            let start = Instant::now();
            let saved = unsafe {
                dr_precision_doc_save_psd(
                    doc,
                    projection,
                    c_path.as_ptr(),
                    0,
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    std::ptr::null(),
                    &mut error,
                )
            };
            let elapsed = ms(start);
            typed_error(error, "typed PSD save");
            assert!(saved, "typed PSD save failed");
            let bytes = std::fs::read(&path).expect("the saved PSD");
            std::fs::remove_file(&path).expect("the saved PSD removed");
            (elapsed, bytes)
        },
        |(doc, projection)| unsafe {
            dr_precision_image_free(projection);
            dr_precision_doc_free(doc);
        },
    );

    let request =
        std::ffi::CString::new(json!({"format":"tiff","bit_depth":32}).to_string()).unwrap();
    unseeded(
        "export_float32",
        "f32",
        FLOAT_DIMENSIONS,
        || {
            let float = float_corpus_document(FLOAT_DIMENSIONS);
            (float, host_projection(float))
        },
        |&(float, projection)| export_encode(float, projection, &request),
        |(float, projection)| unsafe {
            dr_precision_image_free(projection);
            dr_precision_doc_free(float);
        },
    );
}

/// Every raster record's offset and samples of `doc`, as bytes (u16 codes,
/// f32 bits), for `typed/plates`' hashes.
fn plates_document_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..doc.layer_count() {
        let layer = doc.layer(i).expect("a record");
        let (x, y) = layer.offset();
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        if let Some(image) = layer.source() {
            out.extend(plates_image_bytes(image));
        }
    }
    out
}

fn plates_image_bytes(image: &PrecisionImage) -> Vec<u8> {
    match image.to_rgba16() {
        Ok(samples) => samples
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
            .collect(),
        Err(_) => image
            .to_rgba32()
            .expect("f32 samples")
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes()))
            .collect(),
    }
}

/// The 24 MP corpus `v·257` plus a second raster (the corpus pattern at
/// half size, opacity 0.8) at `(−w/6, 2h/3)`, partly off the canvas.
fn plates_document((width, height): (u32, u32)) -> PrecisionDocument {
    let wide = |w: u32, h: u32| -> PrecisionImage {
        let samples: Vec<[u16; 4]> = phase4_corpus::image(w, h)
            .into_raw()
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
            .collect();
        PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None)
            .expect("a typed corpus image")
    };
    let layers = vec![
        Layer::raster("Base", wide(width, height), (0, 0), 1.0, true).expect("the base"),
        Layer::raster(
            "Off",
            wide(width / 2, height / 2),
            (-((width / 6) as i32), (height * 2 / 3) as i32),
            0.8,
            true,
        )
        .expect("the offset raster"),
    ];
    PrecisionDocument::from_parts(
        width,
        height,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("the plates document")
}

fn plates_error(out_is_null: bool, error: *mut c_char) -> Result<(), String> {
    if error.is_null() && !out_is_null {
        return Ok(());
    }
    let message = if error.is_null() {
        "<no error message>".to_string()
    } else {
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        text
    };
    Err(message)
}

fn plates_source(
    doc: &PrecisionDocument,
    layer: usize,
    mode: u32,
) -> Result<PrecisionImage, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_source(
            doc,
            layer,
            mode,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(*unsafe { Box::from_raw(out) })
}

fn plates_edit(
    doc: &PrecisionDocument,
    request: &std::ffi::CStr,
    coverage: &[u16],
) -> Result<PrecisionDocument, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(*unsafe { Box::from_raw(out) })
}

fn plates_select(doc: &PrecisionDocument, request: &std::ffi::CStr) -> Result<Vec<u16>, String> {
    let (w, h) = doc.dimensions();
    let mut out = vec![0_u16; w as usize * h as usize];
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(!ok, error)?;
    Ok(out)
}

fn plates_geometry(doc: &PrecisionDocument, request: &Value) -> Result<PrecisionDocument, String> {
    let edit = GeometryEdit::parse(&request.to_string())?;
    doc.geometry(&edit, REFERENCE_WORKING_BYTES, 0, None)?
        .ok_or_else(|| "no change".to_string())
}

/// Canvas u16 coverage, 65535 inside `(x, y, w, h)`.
fn plates_coverage((width, height): (u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = vec![0_u16; width as usize * height as usize];
    for row in y..y + h {
        let at = row as usize * width as usize + x as usize;
        out[at..at + w as usize].fill(u16::MAX);
    }
    out
}

/// One `typed/plates` line. For each size in turn: the inputs built
/// (untimed), `dr_mixed_resources_evict_caches(NULL, 0, NULL, 0)`, call 1 (`ms_first`),
/// `contiguous_view_bytes()` while its answer lives (`views_held`) and after
/// it is freed (`views_after`), call 2 (`ms_warm`), whose answer `sha256`
/// hashes. A size the core refuses falls through to the next one.
fn plates_line<T, A>(
    name: &str,
    depth: &str,
    sizes: &[(u32, u32)],
    build: impl Fn((u32, u32)) -> T,
    call: impl Fn(&T) -> Result<A, String>,
    answer: impl Fn(&A) -> Vec<u8>,
) {
    timed_line("typed/plates", name, depth, sizes, build, call, answer);
}

/// [`plates_line`]'s measurement under the `op` label (`typed/plates`,
/// `typed/clipboard`).
fn timed_line<T, A>(
    op: &str,
    name: &str,
    depth: &str,
    sizes: &[(u32, u32)],
    build: impl Fn((u32, u32)) -> T,
    call: impl Fn(&T) -> Result<A, String>,
    answer: impl Fn(&A) -> Vec<u8>,
) {
    for (k, &size) in sizes.iter().enumerate() {
        let inputs = build(size);
        resource_report::evict(&[]).expect("an eviction report");
        let start = Instant::now();
        let first = call(&inputs);
        let ms_first = ms(start);
        let first = match first {
            Ok(first) => first,
            Err(e) if k + 1 < sizes.len() => {
                eprintln!("{op} {name}: refused at {size:?} ({e}); next size");
                continue;
            }
            Err(e) => panic!("{op} {name} failed: {e}"),
        };
        let views_held = contiguous_view_bytes();
        drop(first);
        let views_after = contiguous_view_bytes();
        let start = Instant::now();
        let second = call(&inputs).unwrap_or_else(|e| panic!("{op} {name}: {e}"));
        let ms_warm = ms(start);
        let digest = sha256::sha256_hex(&answer(&second));
        drop(second);
        drop(inputs);
        println!(
            "{}",
            json!({
                "op": op,
                "consumer": name,
                "depth": depth,
                "width": size.0,
                "height": size.1,
                "ms_first": ms_first,
                "ms_warm": ms_warm,
                "views_held": views_held,
                "views_after": views_after,
                "sha256": digest,
            })
        );
        return;
    }
}

fn typed_plates() {
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let selection_answer =
        |samples: &Vec<u16>| -> Vec<u8> { samples.iter().flat_map(|v| v.to_le_bytes()).collect() };

    for (name, mode) in [("retouch_source_current", 0), ("retouch_source_below", 1)] {
        plates_line(
            name,
            "u16",
            &sizes,
            plates_document,
            |doc| plates_source(doc, 1, mode),
            plates_image_bytes,
        );
    }
    plates_line(
        "transform_retouch_source",
        "u16",
        &sizes,
        |size| {
            let doc = plates_document(size);
            let plate = plates_source(&doc, 1, 0).expect("the Current plate");
            (doc, plate)
        },
        |(_, plate)| {
            let matrix = [1., 0., 0., 1., 0.5, -0.25];
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_image_transform_retouch_source(
                    plate,
                    matrix.as_ptr(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            plates_error(out.is_null(), error)?;
            Ok(*unsafe { Box::from_raw(out) })
        },
        plates_image_bytes,
    );

    // The clone box `(w/3, h/4, w/10, h/7)`; the Layer clone's matrix maps it
    // onto the offset raster.
    let clone_box = |(w, h): (u32, u32)| (w / 3, h / 4, w / 10, h / 7);
    fn clone_layer((w, h): (u32, u32)) -> Value {
        json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current",
            "matrix":[1,0,0,1,-f64::from(w / 4) - 0.5,f64::from(h / 2) + 0.25],
            "opacity":0.75})
    }
    fn clone_new_layer(_: (u32, u32)) -> Value {
        json!({"op":"retouch_clone","layer":1,"sample":"current_and_below",
            "output":"new_layer","matrix":[1,0,0,1,-3.25,2.5]})
    }
    type Request = fn((u32, u32)) -> Value;
    let clones: [(&str, Request); 2] = [
        ("retouch_clone", clone_layer),
        ("retouch_clone_new_layer", clone_new_layer),
    ];
    for (name, request) in clones {
        plates_line(
            name,
            "u16",
            &sizes,
            |size| {
                let text = std::ffi::CString::new(request(size).to_string()).unwrap();
                (
                    plates_document(size),
                    text,
                    plates_coverage(size, clone_box(size)),
                )
            },
            |(doc, text, coverage)| plates_edit(doc, text, coverage),
            plates_document_bytes,
        );
    }
    plates_line(
        "spot_heal",
        "u16",
        &sizes,
        |size| {
            let (x, y) = (size.0 * 5 / 12, size.1 * 3 / 8);
            let text = std::ffi::CString::new(
                json!({"op":"spot_heal","layer":0,"x":x,"y":y,"width":64,"height":64,
                    "seed":12345})
                .to_string(),
            )
            .unwrap();
            (
                plates_document(size),
                text,
                plates_coverage(size, (x + 16, y + 16, 32, 32)),
            )
        },
        |(doc, text, coverage)| plates_edit(doc, text, coverage),
        plates_document_bytes,
    );

    for (name, source, operation) in [
        (
            "select_luminance_composite",
            "composite",
            json!({"kind":"luminance"}),
        ),
        (
            "select_luminance_layer",
            "layer",
            json!({"kind":"luminance"}),
        ),
        (
            "select_wand",
            "composite",
            json!({"kind":"magic_wand","x":1000,"y":700,"tolerance":3000}),
        ),
        (
            "select_color_range",
            "composite",
            json!({"kind":"color_range","params":{"mode":"sampled",
                "samples":[[30001,30001,30001]],"tolerance":20,"softness":10}}),
        ),
    ] {
        let layer = (source == "layer").then_some(1);
        let text = std::ffi::CString::new(
            json!({"op":"selection","source":source,"layer":layer,"operation":operation})
                .to_string(),
        )
        .unwrap();
        plates_line(
            name,
            "u16",
            &sizes,
            plates_document,
            |doc| plates_select(doc, &text),
            selection_answer,
        );
    }

    plates_line(
        "display_rgba8",
        "f32",
        &[FLOAT_DIMENSIONS],
        |size| {
            let float = unsafe { *Box::from_raw(float_corpus_document(size)) };
            let projection = float
                .render(REFERENCE_WORKING_BYTES, 0, None)
                .expect("a float projection");
            (float, projection)
        },
        |(float, projection)| float.display_rgba8(projection, REFERENCE_WORKING_BYTES, 0, None),
        |image: &image::RgbaImage| image.as_raw().clone(),
    );

    for (name, request) in [
        (
            "geometry_affine_bilinear",
            json!({"op":"affine","layer":1,"target":"pixels",
                "matrix":[0.9,0.2,-0.2,0.9,12.5,-7.25],"filter":"bilinear"}),
        ),
        (
            "geometry_resize_bicubic",
            json!({"op":"resize","width":4500,"height":3000,"filter":"bicubic"}),
        ),
    ] {
        plates_line(
            name,
            "u16",
            &sizes,
            plates_document,
            |doc| plates_geometry(doc, &request),
            plates_document_bytes,
        );
    }
    plates_line(
        "geometry_rotate90_float",
        "f32",
        &[FLOAT_DIMENSIONS],
        |size| unsafe { *Box::from_raw(float_corpus_document(size)) },
        |float| plates_geometry(float, &json!({"op":"rotate90"})),
        plates_document_bytes,
    );

    let request = std::ffi::CString::new(
        json!({"format":"png","bit_depth":16,"size":{"mode":"scale","value":0.5}}).to_string(),
    )
    .unwrap();
    plates_line(
        "export_png16_half",
        "u16",
        &sizes,
        |size| {
            let doc = plates_document(size);
            let projection = doc
                .render(REFERENCE_WORKING_BYTES, 0, None)
                .expect("a projection");
            (doc, projection)
        },
        |(doc, projection)| Ok::<_, String>(export_encode(doc, projection, &request).1),
        |bytes: &Vec<u8>| bytes.clone(),
    );
}

/// Copy / Copy Merged through `dr_precision_doc_clipboard_copy`, no
/// coverage.
fn clipboard_copy_handle(
    doc: &PrecisionDocument,
    request: &std::ffi::CStr,
) -> Result<Box<DrClipboardPayload>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(unsafe { Box::from_raw(out) })
}

/// A payload handle's RZCB bytes (`dr_clipboard_payload_bytes`).
fn clipboard_stream(handle: &DrClipboardPayload) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_clipboard_payload_bytes(handle, &mut length) };
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

type ClipboardCut = (Box<DrClipboardPayload>, Option<PrecisionDocument>);

fn typed_clipboard() {
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let whole = |(w, h): (u32, u32), request: Value| {
        let mut request = request;
        request["x"] = json!(0);
        request["y"] = json!(0);
        request["width"] = json!(w);
        request["height"] = json!(h);
        std::ffi::CString::new(request.to_string()).unwrap()
    };

    for (name, request) in [
        ("copy_layer", json!({"op":"copy","layer":0})),
        ("copy_merged", json!({"op":"copy_merged"})),
    ] {
        timed_line(
            "typed/clipboard",
            name,
            "u16",
            &sizes,
            |size| (plates_document(size), whole(size, request.clone())),
            |(doc, text)| clipboard_copy_handle(doc, text),
            |handle| clipboard_stream(handle),
        );
    }
    timed_line(
        "typed/clipboard",
        "copy_layer_float",
        "f32",
        &[FLOAT_DIMENSIONS],
        |size| {
            let float = unsafe { *Box::from_raw(float_corpus_document(size)) };
            (float, whole(size, json!({"op":"copy","layer":0})))
        },
        |(float, text)| clipboard_copy_handle(float, text),
        |handle| clipboard_stream(handle),
    );
    timed_line(
        "typed/clipboard",
        "cut_layer",
        "u16",
        &sizes,
        |size| {
            (
                plates_document(size),
                whole(size, json!({"op":"cut","layer":0})),
            )
        },
        |(doc, text)| -> Result<ClipboardCut, String> {
            let mut payload = ptr::null_mut();
            let mut error = ptr::null_mut();
            let next = unsafe {
                dr_precision_doc_clipboard_cut(
                    doc,
                    text.as_ptr(),
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut payload,
                    &mut error,
                )
            };
            plates_error(payload.is_null(), error)?;
            Ok((
                unsafe { Box::from_raw(payload) },
                (!next.is_null()).then(|| unsafe { *Box::from_raw(next) }),
            ))
        },
        |(handle, next): &ClipboardCut| {
            let mut out = clipboard_stream(handle);
            match next {
                Some(next) => out.extend(plates_document_bytes(next)),
                None => out.extend(b"none"),
            }
            out
        },
    );
    timed_line(
        "typed/clipboard",
        "paste_convert",
        "u16",
        &sizes,
        |(w, h)| {
            let p3 = IccProfile::display_p3();
            let samples: Vec<[u16; 4]> = phase4_corpus::image(w, h)
                .into_raw()
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
                .collect();
            let image = PrecisionImage::from_rgba16(w, h, samples, p3.clone(), None)
                .expect("a P3 corpus image");
            let source = PrecisionDocument::from_parts(
                w,
                h,
                p3,
                Metadata::default(),
                Resolution::default(),
                vec![Layer::raster("P3", image, (0, 0), 1.0, true).expect("the P3 raster")],
            )
            .expect("the P3 document");
            let read = ClipboardRead::Copy {
                layer: 0,
                x: 0,
                y: 0,
                width: w,
                height: h,
            };
            let payload = source
                .clipboard_copy(&read, None, REFERENCE_WORKING_BYTES, 0, None)
                .expect("a P3 payload");
            drop(source);
            (plates_document((w, h)), payload)
        },
        |(doc, payload)| {
            let request = PasteRequest {
                name: "Pasted".into(),
                anchor: 0,
                position: InsertPosition::Top,
                placement: PastePlacement::InPlace,
                profile: PasteProfile::Convert,
                frame: None,
            };
            doc.clipboard_paste(payload, &request, REFERENCE_WORKING_BYTES, 0, None)
        },
        plates_document_bytes,
    );
}

/// Canvas-sized u16 coverage `(i·7919) mod 65536` for `typed/producers`.
fn producer_coverage((width, height): (u32, u32)) -> Vec<u16> {
    (0..u64::from(width) * u64::from(height))
        .map(|i| ((i * 7919) % 65536) as u16)
        .collect()
}

/// `dr_precision_doc_edit` of `request` (no coverage) for `typed/producers`.
fn producer_edit(doc: &PrecisionDocument, request: &Value) -> Result<PrecisionDocument, String> {
    let text = std::ffi::CString::new(request.to_string()).unwrap();
    plates_edit(doc, &text, &[])
}

/// The 13B-3d-3a lines (`DR_TILE_SHARING_ONLY=producers`): `typed/producers`
/// and `typed/producer_consumers`.
fn typed_producers() {
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let float = [FLOAT_DIMENSIONS];
    let structure = [
        ("merge_down", json!({"op":"merge_down","layer":1})),
        ("merge_visible", json!({"op":"merge_visible"})),
        (
            "stamp_visible",
            json!({"op":"stamp_visible","name":"Stamp","anchor":1}),
        ),
        ("flatten", json!({"op":"flatten"})),
    ];
    for (name, request) in &structure {
        timed_line(
            "typed/producers",
            name,
            "u16",
            &sizes,
            plates_document,
            |doc| producer_edit(doc, request),
            plates_document_bytes,
        );
    }
    timed_line(
        "typed/producers",
        "rasterize",
        "u16",
        &sizes,
        |size| (plates_document(size), producer_coverage(size)),
        |(doc, coverage)| {
            let (w, h) = doc.dimensions();
            let request = std::ffi::CString::new(
                json!({"op":"rasterize","name":"Shape","width":w,"height":h,"x":0,"y":0,
                    "colors":[[0.25,0.5,0.75,1.0]],"color_space":"document"})
                .to_string(),
            )
            .unwrap();
            plates_edit(doc, &request, coverage)
        },
        plates_document_bytes,
    );
    timed_line(
        "typed/producers",
        "artwork",
        "u16",
        &sizes,
        |size| (size, producer_coverage(size), IccProfile::srgb()),
        |((w, h), coverage, profile)| {
            let description =
                std::ffi::CString::new(r#"[{"kind":"coverage","color":[1,0,0,1]}]"#).unwrap();
            let mut error = ptr::null_mut();
            let image = unsafe {
                dr_artwork_render(
                    *w,
                    *h,
                    description.as_ptr(),
                    coverage.as_ptr(),
                    coverage.len(),
                    ptr::null(),
                    0,
                    profile.bytes().as_ptr(),
                    profile.bytes().len(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            plates_error(image.is_null(), error)?;
            Ok(*unsafe { Box::from_raw(image) })
        },
        plates_image_bytes,
    );
    let p3 = {
        let len = dr_builtin_profile_len(1);
        let mut out = vec![0u8; len];
        assert!(unsafe { dr_builtin_profile(1, out.as_mut_ptr(), len) });
        out
    };
    let options = std::ffi::CString::new(r#"{"adjustments":"preserve_appearance"}"#).unwrap();
    let invert =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    timed_line(
        "typed/producers",
        "bake",
        "u16",
        &sizes,
        |size| {
            let base = *unsafe { Box::from_raw(typed_base(size)) };
            base.add_adjustment("Invert", &invert, 1.0, REFERENCE_WORKING_BYTES, 0, None)
                .expect("an adjustment")
                .expect("a changed document")
        },
        |doc| {
            let mut error = ptr::null_mut();
            let mut report = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_convert_to_profile_with(
                    doc,
                    p3.as_ptr(),
                    p3.len(),
                    options.as_ptr(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut report,
                    &mut error,
                )
            };
            if !report.is_null() {
                unsafe { dr_string_free(report) };
            }
            plates_error(out.is_null(), error)?;
            Ok(*unsafe { Box::from_raw(out) })
        },
        plates_document_bytes,
    );
    timed_line(
        "typed/producers",
        "warp",
        "u16",
        &float,
        plates_document,
        |doc| {
            doc.warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())
        },
        plates_document_bytes,
    );
    timed_line(
        "typed/producers",
        "float_to_u16",
        "f32",
        &float,
        |size| *unsafe { Box::from_raw(float_corpus_document(size)) },
        |doc| doc.float_to_u16(REFERENCE_WORKING_BYTES, 0, None),
        plates_document_bytes,
    );
    timed_line(
        "typed/producers",
        "narrow_to_u16",
        "f32",
        &float,
        |size| *unsafe { Box::from_raw(float_corpus_document(size)) },
        |doc| doc.narrow_structure_to_u16(REFERENCE_WORKING_BYTES, 0, None),
        plates_document_bytes,
    );
    producer_consumer(
        "warp_after_flatten",
        FLOAT_DIMENSIONS,
        |flat| {
            flat.warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())
        },
        plates_document_bytes,
    );
    producer_consumer(
        "save_after_flatten",
        phase4_corpus::DIMENSIONS[0],
        |flat| flat.encode_native(REFERENCE_WORKING_BYTES, 0, None),
        |bytes: &Vec<u8>| bytes.clone(),
    );
}

/// One `typed/producer_consumers` line: `typed/plates`' document built, the
/// views evicted, flattened (untimed), `views_before`; `consumer` timed
/// twice with NO eviction in between (`ms_first`, the first answer dropped,
/// `ms_warm`); `views_held` while the second answer lives, which `sha256`
/// hashes.
fn producer_consumer<A>(
    name: &str,
    size: (u32, u32),
    consumer: impl Fn(&PrecisionDocument) -> Result<A, String>,
    answer: impl Fn(&A) -> Vec<u8>,
) {
    let doc = plates_document(size);
    resource_report::evict(&[]).expect("an eviction report");
    let flat = producer_edit(&doc, &json!({"op":"flatten"})).expect("a flattened document");
    let views_before = contiguous_view_bytes();
    let start = Instant::now();
    let first = consumer(&flat).unwrap_or_else(|e| panic!("{name}: {e}"));
    let ms_first = ms(start);
    drop(first);
    let start = Instant::now();
    let second = consumer(&flat).unwrap_or_else(|e| panic!("{name}: {e}"));
    let ms_warm = ms(start);
    let views_held = contiguous_view_bytes();
    let digest = sha256::sha256_hex(&answer(&second));
    drop(second);
    drop((flat, doc));
    println!(
        "{}",
        json!({
            "op": "typed/producer_consumers",
            "consumer": name,
            "width": size.0,
            "height": size.1,
            "ms_first": ms_first,
            "ms_warm": ms_warm,
            "views_before": views_before,
            "views_held": views_held,
            "sha256": digest,
        })
    );
}

/// One `typed/filled` line: [`timed_line`]'s procedure at one size, plus
/// `layer(&answer 2)`'s `(layer_retained_bytes, layer_plane_bytes)`.
fn filled_line<T, A>(
    name: &str,
    depth: &str,
    size: (u32, u32),
    build: impl Fn((u32, u32)) -> T,
    call: impl Fn(&T) -> Result<A, String>,
    answer: impl Fn(&A) -> Vec<u8>,
    layer: impl Fn(&A) -> (u64, u64),
) {
    let inputs = build(size);
    resource_report::evict(&[]).expect("an eviction report");
    let start = Instant::now();
    let first = call(&inputs).unwrap_or_else(|e| panic!("typed/filled {name}: {e}"));
    let ms_first = ms(start);
    let views_held = contiguous_view_bytes();
    drop(first);
    let views_after = contiguous_view_bytes();
    let start = Instant::now();
    let second = call(&inputs).unwrap_or_else(|e| panic!("typed/filled {name}: {e}"));
    let ms_warm = ms(start);
    let (layer_retained_bytes, layer_plane_bytes) = layer(&second);
    let digest = sha256::sha256_hex(&answer(&second));
    drop(second);
    drop(inputs);
    println!(
        "{}",
        json!({
            "op": "typed/filled",
            "consumer": name,
            "depth": depth,
            "width": size.0,
            "height": size.1,
            "ms_first": ms_first,
            "ms_warm": ms_warm,
            "views_held": views_held,
            "views_after": views_after,
            "layer_retained_bytes": layer_retained_bytes,
            "layer_plane_bytes": layer_plane_bytes,
            "sha256": digest,
        })
    );
}

/// The added (top) layer's `retained_bytes()` and its plane's report bytes.
fn added_layer_bytes(doc: &PrecisionDocument) -> (u64, u64) {
    let image = doc
        .layer_source(doc.layer_count() - 1)
        .expect("the added raster");
    let report = typed_report(&[], &[image as *const PrecisionImage]);
    let plane = category(&report, "precision_pixels", "unique_bytes")
        + category(&report, "tile_structure", "unique_bytes");
    (image.retained_bytes(), plane)
}

/// Every byte layer's offset, RGBA8 bytes and mask bytes.
fn narrowed_bytes(doc: &DrDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for layer in &doc.layers {
        out.extend(layer.offset.0.to_le_bytes());
        out.extend(layer.offset.1.to_le_bytes());
        out.extend(
            layer
                .pixels
                .to_contiguous()
                .expect("the layer's pixels")
                .into_raw(),
        );
        if let Some(mask) = &layer.mask {
            out.extend(mask.as_raw());
        }
    }
    out
}

/// The 13B-3d-3b line (`DR_TILE_SHARING_ONLY=filled`): `typed/filled`.
fn typed_filled() {
    let add_layer = |doc: &PrecisionDocument| {
        doc.add_layer_at("Empty", 0, InsertPosition::Top)?
            .ok_or_else(|| "no change".to_string())
    };
    filled_line(
        "add_layer",
        "u16",
        phase4_corpus::DIMENSIONS[0],
        |size| *unsafe { Box::from_raw(typed_base(size)) },
        add_layer,
        plates_document_bytes,
        added_layer_bytes,
    );
    filled_line(
        "add_layer",
        "f32",
        FLOAT_DIMENSIONS,
        |size| *unsafe { Box::from_raw(float_corpus_document(size)) },
        add_layer,
        plates_document_bytes,
        added_layer_bytes,
    );
    let invert =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    filled_line(
        "narrow_masked_adjustment",
        "u16",
        phase4_corpus::DIMENSIONS[0],
        |size| {
            let base = *unsafe { Box::from_raw(typed_base(size)) };
            let doc = base
                .add_adjustment("Invert", &invert, 1.0, REFERENCE_WORKING_BYTES, 0, None)
                .expect("an adjustment")
                .expect("a changed document");
            let mask = MaskEdit::parse(
                &json!({"op":"set_mask","layer":1,"width":size.0,"height":size.1,"x":0,"y":0})
                    .to_string(),
            )
            .expect("a mask request");
            doc.mask_edit(
                &mask,
                &producer_coverage(size),
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .expect("a mask")
            .expect("a masked document")
        },
        |doc| doc.narrow_structure_to_u8(PrecisionDither::None, REFERENCE_WORKING_BYTES, 0, None),
        narrowed_bytes,
        |doc| {
            let pixels = category(
                &report(&[doc as *const DrDocument]),
                "layer_pixels",
                "unique_bytes",
            );
            (pixels, pixels)
        },
    );
}

/// `dr_precision_doc_export_encode` of `projection` (NULL: the document
/// renders its own) under `request`, answering the encoded bytes or the
/// refusal.
fn exports_encode(
    doc: &PrecisionDocument,
    projection: Option<&PrecisionImage>,
    request: &std::ffi::CStr,
) -> Result<Vec<u8>, String> {
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            projection.map_or(ptr::null(), |p| p as *const _),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(result.is_null(), error)?;
    let mut length = 0_usize;
    let bytes = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!bytes.is_null(), "export bytes");
    let answer = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    Ok(answer)
}

/// Runs `write` against a temporary path, answering the written file's
/// bytes (the file deleted).
fn exports_file(
    name: &str,
    write: impl FnOnce(&std::ffi::CStr, &mut *mut c_char) -> bool,
) -> Result<Vec<u8>, String> {
    let path = std::env::temp_dir().join(format!("phase13b_exports_{}_{name}", std::process::id()));
    let c_path = std::ffi::CString::new(path.to_str().expect("a UTF-8 path")).unwrap();
    let mut error = ptr::null_mut();
    let ok = write(&c_path, &mut error);
    plates_error(!ok, error)?;
    let bytes = std::fs::read(&path).expect("the written file");
    std::fs::remove_file(&path).expect("the written file removed");
    Ok(bytes)
}

/// `dr_precision_doc_save_psd` of `doc` and `projection` in `mode`.
fn exports_psd(
    doc: &PrecisionDocument,
    projection: &PrecisionImage,
    mode: i32,
) -> Result<Vec<u8>, String> {
    exports_file("save.psd", |path, error| unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection,
            path.as_ptr(),
            0,
            mode,
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            error,
        )
    })
}

/// The 13B-3e-1 line (`DR_TILE_SHARING_ONLY=exports`): `typed/exports`.
fn typed_exports() {
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let bytes = |answer: &Vec<u8>| answer.clone();
    let with_projection = |size: (u32, u32)| {
        let doc = plates_document(size);
        let projection = doc
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .expect("a projection");
        (doc, projection)
    };
    let sharpen = r#""sharpen":{"radius":1.4,"amount":1.75,"threshold":0.0}"#;
    for (name, request) in [
        (
            "export_png16_sharpen",
            format!(r#"{{"format":"png","bit_depth":16,{sharpen}}}"#),
        ),
        (
            "export_tiff16_p3",
            r#"{"format":"tiff","bit_depth":16,"profile":"display-p3"}"#.to_string(),
        ),
    ] {
        let request = std::ffi::CString::new(request).unwrap();
        timed_line(
            "typed/exports",
            name,
            "u16",
            &sizes,
            with_projection,
            |(doc, projection)| exports_encode(doc, Some(projection), &request),
            bytes,
        );
    }
    let png16 = std::ffi::CString::new(r#"{"format":"png","bit_depth":16}"#).unwrap();
    timed_line(
        "typed/exports",
        "export_png16_render",
        "u16",
        &sizes,
        plates_document,
        |doc| exports_encode(doc, None, &png16),
        bytes,
    );
    let layer_request =
        ExportRequest::parse(r#"{"format":"png","bit_depth":16}"#).expect("a PNG16 request");
    timed_line(
        "typed/exports",
        "export_layer_trimmed",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            doc.export_layer_encoded(1, true, &layer_request, REFERENCE_WORKING_BYTES, 0, None)
                .map(|result| result.bytes().to_vec())
        },
        bytes,
    );
    timed_line(
        "typed/exports",
        "export16_png",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            exports_file("export16.png", |path, error| unsafe {
                let mut carried = 0_u32;
                dr_precision_doc_export16(
                    doc,
                    path.as_ptr(),
                    0,
                    true,
                    false,
                    REFERENCE_WORKING_BYTES,
                    0,
                    &mut carried,
                    std::ptr::null(),
                    error,
                )
            })
        },
        bytes,
    );
    timed_line(
        "typed/exports",
        "save_psd_flattened",
        "u16",
        &sizes,
        with_projection,
        |(doc, projection)| exports_psd(doc, projection, 1),
        bytes,
    );
    let stroke = json!({"op":"layer_style","version":1,"layer":1,
        "style":{"effects":[{"type":"stroke","size":0.5}]}});
    timed_line(
        "typed/exports",
        "save_psd_layers_baked",
        "u16",
        &sizes,
        |size| {
            let doc = producer_edit(&plates_document(size), &stroke).expect("a styled document");
            let projection = doc
                .render(REFERENCE_WORKING_BYTES, 0, None)
                .expect("a projection");
            (doc, projection)
        },
        |(doc, projection)| exports_psd(doc, projection, 0),
        bytes,
    );

    let float = [FLOAT_DIMENSIONS];
    let float_with_projection = |size: (u32, u32)| {
        let doc = unsafe { *Box::from_raw(float_corpus_document(size)) };
        let projection = doc
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .expect("a float projection");
        (doc, projection)
    };
    for (name, request) in [
        ("export_float32", r#"{"format":"tiff","bit_depth":32}"#),
        ("export_float_png16", r#"{"format":"png","bit_depth":16}"#),
    ] {
        let request = std::ffi::CString::new(request).unwrap();
        timed_line(
            "typed/exports",
            name,
            "f32",
            &float,
            float_with_projection,
            |(doc, projection)| exports_encode(doc, Some(projection), &request),
            bytes,
        );
    }
    timed_line(
        "typed/exports",
        "save_native_float",
        "f32",
        &float,
        |size| unsafe { *Box::from_raw(float_corpus_document(size)) },
        |doc| {
            // The trailing `LIDS` ids are process-minted: zeroed.
            let mut stream = doc.encode_native(REFERENCE_WORKING_BYTES, 0, None)?;
            let start = stream.len().saturating_sub(8 * doc.layer_count());
            stream[start..].fill(0);
            Ok(stream)
        },
        bytes,
    );
}

fn main() {
    match std::env::var("DR_TILE_SHARING_ONLY").as_deref() {
        Ok("decoders") => {
            typed_decoders();
            return;
        }
        Ok("raw_intake") => {
            typed_raw_intake();
            return;
        }
        Ok("raw_develop") => {
            typed_raw_develop();
            return;
        }
        Ok("whole_readers") => {
            typed_whole_readers();
            return;
        }
        Ok("exports") => {
            typed_exports();
            return;
        }
        Ok("filled") => {
            typed_filled();
            return;
        }
        Ok("retouch") => {
            typed_retouch();
            return;
        }
        Ok("producers") => {
            typed_producers();
            return;
        }
        Ok("dab") => {
            for handle in typed_dab_lines(phase4_corpus::DIMENSIONS[0])
                .into_iter()
                .rev()
            {
                unsafe { dr_precision_doc_free(handle) };
            }
            return;
        }
        Ok("typed") => {
            typed(phase4_corpus::DIMENSIONS[0]);
            typed_float_projection(FLOAT_DIMENSIONS);
            typed_promote(phase4_corpus::DIMENSIONS[0]);
            typed_consumers();
            typed_unseeded();
            typed_plates();
            typed_clipboard();
            return;
        }
        Ok("consumers") => {
            typed_consumers();
            return;
        }
        Ok("unseeded") => {
            typed_unseeded();
            return;
        }
        Ok("plates") => {
            typed_plates();
            return;
        }
        Ok("clipboard") => {
            typed_clipboard();
            return;
        }
        _ => {}
    }
    for (width, height) in phase4_corpus::DIMENSIONS {
        let size = (width, height);
        let source: Vec<[u8; 4]> = phase4_corpus::image(width, height)
            .into_raw()
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();

        let start = Instant::now();
        let plane = TilePlane::from_contiguous(width, height, &source, None).expect("tiled");
        line("from_contiguous", size, 1, accounting(&[&plane]), ms(start));

        let start = Instant::now();
        let back = plane.to_contiguous(None).expect("contiguous");
        let elapsed = ms(start);
        assert!(back == source, "the round trip is exact");
        line("to_contiguous", size, 1, accounting(&[&plane]), elapsed);

        let start = Instant::now();
        let filled = TilePlane::filled(width, height, [1_u8, 2, 3, 4]).expect("filled");
        line("filled", size, 1, accounting(&[&filled]), ms(start));

        let dab = vec![[0_u8; 4]; 300 * 300];
        let mut states = vec![plane];
        let start = Instant::now();
        for i in 0..8 {
            let next = states[i]
                .with_rect(100 + 512 * i as u32, 100, 300, 300, &dab, None)
                .expect("an edit")
                .expect("a changed plane");
            states.push(next);
        }
        let elapsed = ms(start);
        let refs: Vec<_> = states.iter().collect();
        line("dab_sequence", size, 9, accounting(&refs), elapsed);
        drop(states);

        document_dab(size);
    }
    flattened(phase4_corpus::DIMENSIONS[0]);
    regional_window(phase4_corpus::DIMENSIONS[0]);
    rect_writers(phase4_corpus::DIMENSIONS[0]);
    placed_source(phase4_corpus::DIMENSIONS[0]);
    transforms(phase4_corpus::DIMENSIONS[0]);
    typed(phase4_corpus::DIMENSIONS[0]);
    typed_float_projection(FLOAT_DIMENSIONS);
    typed_promote(phase4_corpus::DIMENSIONS[0]);
    typed_consumers();
    typed_unseeded();
    typed_plates();
    typed_clipboard();
    typed_retouch();
    typed_producers();
    typed_filled();
    typed_exports();

    typed_whole_readers();
    typed_raw_develop();
    typed_raw_intake();
    typed_decoders();
}

/// An FFI call's owned string answer as bytes (freed), or its error.
fn whole_text(result: *mut c_char, error: *mut c_char) -> Result<Vec<u8>, String> {
    plates_error(result.is_null(), error)?;
    let bytes = unsafe { CStr::from_ptr(result) }.to_bytes().to_vec();
    unsafe { dr_string_free(result) };
    Ok(bytes)
}

/// `dr_precision_doc_edit_plan`'s JSON.
fn whole_plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = std::ffi::CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit_plan(doc, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut error)
    };
    Ok(serde_json::from_slice(&whole_text(out, error)?).expect("plan JSON"))
}

/// The 13B-3e-3a lines (`DR_TILE_SHARING_ONLY=whole_readers`).
fn typed_whole_readers() {
    const OP: &str = "typed/whole_readers";
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let bytes = |answer: &Vec<u8>| answer.clone();
    timed_line(
        OP,
        "selected_pixelate",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            let edit = SelectedEdit::parse(r#"{"layer":0,"op":"pixelate","params":{"block":8}}"#)?;
            doc.selected_edit(&edit, &[], REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())
        },
        plates_document_bytes,
    );
    timed_line(
        OP,
        "selected_auto_color",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            let request = std::ffi::CString::new(
                r#"{"layer":0,"op":"auto_color","params":{"clip":0.001},"analysis_scope":"whole_target"}"#,
            )
            .unwrap();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_selected_auto_levels(
                    doc,
                    request.as_ptr(),
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            whole_text(out, error)
        },
        bytes,
    );
    timed_line(
        OP,
        "read_plane_green",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            let request = json!({"op":"read_plane","target":"green","index":0,"canvas":false});
            let n = whole_plan(doc, &request)?["output_samples"]
                .as_u64()
                .expect("output_samples") as usize;
            let mut out = vec![0_u16; n];
            let text = std::ffi::CString::new(request.to_string()).unwrap();
            let mut error = ptr::null_mut();
            let ok = unsafe {
                dr_precision_doc_read_plane(
                    doc,
                    text.as_ptr(),
                    out.as_mut_ptr(),
                    out.len(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            plates_error(!ok, error)?;
            Ok(out
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>())
        },
        bytes,
    );
    timed_line(
        OP,
        "refinement_cutout",
        "u16",
        &sizes,
        |(w, h)| {
            let samples: Vec<[u16; 4]> = phase4_corpus::image(w, h)
                .into_raw()
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
                .collect();
            let foreground = PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None)
                .expect("the corpus foreground");
            let matte = plates_coverage((w, h), (w / 4, h / 4, w / 2, h / 2));
            (plates_document((w, h)), foreground, matte)
        },
        |(doc, foreground, matte)| {
            doc.add_refinement_cutout(foreground, matte, REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())
        },
        plates_document_bytes,
    );
    let scalar = |doc: &PrecisionDocument, request: Value| -> Result<PrecisionDocument, String> {
        let n = whole_plan(doc, &request)?["input_samples"]
            .as_u64()
            .expect("input_samples") as usize;
        let input = vec![0_u16; n];
        let text = std::ffi::CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                doc,
                text.as_ptr(),
                input.as_ptr(),
                input.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        plates_error(out.is_null(), error)?;
        Ok(*unsafe { Box::from_raw(out) })
    };
    timed_line(
        OP,
        "scalar_pixelate",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            scalar(
                doc,
                json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
                    "operation":{"kind":"filter","edit":{"layer":0,"op":"pixelate",
                    "params":{"block":8}}}}),
            )
        },
        plates_document_bytes,
    );
    timed_line(
        OP,
        "scalar_auto_tone",
        "u16",
        &sizes,
        plates_document,
        |doc| {
            let request = std::ffi::CString::new(
                json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
                    "operation":{"kind":"filter","edit":{"layer":0,"op":"auto_tone",
                    "params":{"clip":0.001}}}})
                .to_string(),
            )
            .unwrap();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_scalar_auto_levels(
                    doc,
                    request.as_ptr(),
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            whole_text(out, error)
        },
        bytes,
    );
    // The warp's work limit refuses 24 MP: 3000×2000 only.
    let warp_sizes = [FLOAT_DIMENSIONS];
    timed_line(
        OP,
        "warp_edit",
        "u16",
        &warp_sizes,
        plates_document,
        |doc| {
            doc.warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())
        },
        plates_document_bytes,
    );
    timed_line(
        OP,
        "warp_save",
        "u16",
        &warp_sizes,
        |size| {
            plates_document(size)
                .warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)
                .expect("a warp")
                .expect("a warped document")
        },
        |doc| {
            // The trailing `LIDS` ids are process-minted: zeroed.
            let mut stream = doc.encode_native(REFERENCE_WORKING_BYTES, 0, None)?;
            let start = stream.len().saturating_sub(8 * doc.layer_count());
            stream[start..].fill(0);
            Ok(stream)
        },
        bytes,
    );
}

/// One plain sRGB raster of the corpus `v·257`.
fn raw_corpus_document((w, h): (u32, u32), invert: bool) -> PrecisionDocument {
    let samples: Vec<[u16; 4]> = phase4_corpus::image(w, h)
        .into_raw()
        .chunks_exact(4)
        .map(|p| {
            let v = [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257);
            if invert {
                [65535 - v[0], 65535 - v[1], 65535 - v[2], v[3]]
            } else {
                v
            }
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None)
        .expect("a typed corpus raster");
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
        .expect("a plain corpus document")
}

/// An RZLM mask, 65535 in the centred half `[w/4, 3w/4) × [h/4, 3h/4)`.
fn raw_centred_mask((w, h): (u32, u32)) -> RawLocalMask {
    let mut bytes = Vec::with_capacity(16 + w as usize * h as usize * 2);
    bytes.extend_from_slice(b"RZLM");
    for v in [1, w, h] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in plates_coverage((w, h), (w / 4, h / 4, w / 2, h / 2)) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    RawLocalMask::from_blob(std::sync::Arc::new(bytes)).expect("an RZLM mask")
}

/// `dr_precision_image_develop_preview` of layer 0.
fn raw_preview(
    doc: &PrecisionDocument,
    region: [u32; 4],
    divisor: u32,
    clipping: u32,
) -> Result<Box<DrImage>, String> {
    let image = doc.layer_source(0).expect("a layer source");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_develop_preview(
            image,
            region[0],
            region[1],
            region[2],
            region[3],
            divisor,
            clipping,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(unsafe { Box::from_raw(out) })
}

/// A preview's width, height (u32 LE) and RGBA8 bytes.
fn raw_preview_bytes(image: &DrImage) -> Vec<u8> {
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let mut out = [w.to_le_bytes(), h.to_le_bytes()].concat();
    out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
    });
    out
}

/// [`timed_line`] with `ms_warm` the median of 9 consecutive warm calls
/// (the last one hashed) and `ms_warm_min` their minimum.
fn timed_line_warm9<T, A>(
    op: &str,
    name: &str,
    depth: &str,
    sizes: &[(u32, u32)],
    build: impl Fn((u32, u32)) -> T,
    call: impl Fn(&T) -> Result<A, String>,
    answer: impl Fn(&A) -> Vec<u8>,
) {
    for (k, &size) in sizes.iter().enumerate() {
        let inputs = build(size);
        resource_report::evict(&[]).expect("an eviction report");
        let start = Instant::now();
        let first = call(&inputs);
        let ms_first = ms(start);
        let first = match first {
            Ok(first) => first,
            Err(e) if k + 1 < sizes.len() => {
                eprintln!("{op} {name}: refused at {size:?} ({e}); next size");
                continue;
            }
            Err(e) => panic!("{op} {name} failed: {e}"),
        };
        let views_held = contiguous_view_bytes();
        drop(first);
        let views_after = contiguous_view_bytes();
        let mut warm = Vec::with_capacity(9);
        let mut digest = String::new();
        for _ in 0..9 {
            let start = Instant::now();
            let second = call(&inputs).unwrap_or_else(|e| panic!("{op} {name}: {e}"));
            warm.push(ms(start));
            digest = sha256::sha256_hex(&answer(&second));
        }
        drop(inputs);
        warm.sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({
                "op": op,
                "consumer": name,
                "depth": depth,
                "width": size.0,
                "height": size.1,
                "ms_first": ms_first,
                "ms_warm": warm[4],
                "ms_warm_min": warm[0],
                "views_held": views_held,
                "views_after": views_after,
                "sha256": digest,
            })
        );
        return;
    }
}

/// The 13B-3e-3b lines (`DR_TILE_SHARING_ONLY=raw_develop`).
fn typed_raw_develop() {
    const OP: &str = "typed/raw_develop";
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let base = |size| raw_corpus_document(size, false);
    let run_develop = |base: &PrecisionDocument,
                       variants: &[&PrecisionDocument],
                       masks: &[&RawLocalMask],
                       settings: &str,
                       vignette: Option<&str>|
     -> Result<PrecisionDocument, String> {
        develop(
            base,
            &variants.iter().map(|v| Some(*v)).collect::<Vec<_>>(),
            masks,
            settings,
            vignette,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )?
        .ok_or_else(|| "no change".to_string())
    };
    timed_line(
        OP,
        "develop_vignette",
        "u16",
        &sizes,
        |(w, h)| {
            let vignette = json!({"stops":-1.0,"center":[f64::from(w) / 2.0, f64::from(h) / 2.0],
                "falloff":1.0})
            .to_string();
            (base((w, h)), vignette)
        },
        |(doc, vignette)| run_develop(doc, &[], &[], "[]", Some(vignette)),
        plates_document_bytes,
    );
    timed_line(
        OP,
        "develop_local",
        "u16",
        &sizes,
        |size| {
            (
                base(size),
                raw_corpus_document(size, true),
                raw_centred_mask(size),
            )
        },
        |(doc, variant, mask)| {
            run_develop(
                doc,
                &[variant],
                &[mask],
                r#"[{"enabled":true,"density":0.75,"feather":0}]"#,
                None,
            )
        },
        plates_document_bytes,
    );
    for (name, request) in [
        (
            "geometry_exif6",
            json!({"version":1,"kind":"exif","orientation":6}),
        ),
        (
            "geometry_lens_bilinear",
            json!({"version":1,"kind":"lens","profile_id":"bench","center":[0.5,0.5],
                "radial":[0.02,0.0,0.0],"red_scale":1.0,"blue_scale":1.0,"filter":"bilinear"}),
        ),
    ] {
        let request = request.to_string();
        timed_line(
            OP,
            name,
            "u16",
            &sizes,
            base,
            |doc| {
                doc.raw_source_geometry(&request, REFERENCE_WORKING_BYTES, 0, None)?
                    .ok_or_else(|| "no change".to_string())
            },
            plates_document_bytes,
        );
    }
    for (name, divisor, clipping, detail) in [
        ("preview_fit", 4, 0, false),
        ("preview_fit_clipping", 4, 3, false),
        ("preview_detail", 1, 3, true),
    ] {
        timed_line_warm9(
            OP,
            name,
            "u16",
            &sizes,
            base,
            |doc| {
                let (w, h) = doc.dimensions();
                let region = if detail {
                    [(w - 1024) / 2, (h - 1024) / 2, 1024, 1024]
                } else {
                    [0, 0, w, h]
                };
                raw_preview(doc, region, divisor, clipping)
            },
            |image| raw_preview_bytes(image),
        );
    }
}

/// The corpus `v·257` as a host sample buffer.
fn raw_intake_samples((w, h): (u32, u32)) -> Vec<[u16; 4]> {
    phase4_corpus::image(w, h)
        .into_raw()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect()
}

/// `dr_precision_doc_from_rgba16` over host samples.
fn raw_intake_rgba16(
    samples: &[[u16; 4]],
    (w, h): (u32, u32),
) -> Result<PrecisionDocument, String> {
    let profile = IccProfile::srgb();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr().cast(),
            samples.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(*unsafe { Box::from_raw(out) })
}

/// A `typed/raw_intake` answer: every document the call made, in order,
/// and the preview when there is one.
struct RawIntake {
    docs: Vec<PrecisionDocument>,
    preview: Option<Box<DrImage>>,
}

fn raw_intake_bytes(answer: &RawIntake) -> Vec<u8> {
    let mut out = Vec::new();
    for doc in &answer.docs {
        out.extend(plates_document_bytes(doc));
    }
    if let Some(preview) = &answer.preview {
        out.extend(raw_preview_bytes(preview));
    }
    out
}

/// The 13B-3e-4a line (`DR_TILE_SHARING_ONLY=raw_intake`): `typed/raw_intake`.
fn typed_raw_intake() {
    const OP: &str = "typed/raw_intake";
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let host = |size| (size, raw_intake_samples(size));
    let intake = |(size, samples): &((u32, u32), Vec<[u16; 4]>)| raw_intake_rgba16(samples, *size);
    let only = |docs: Vec<PrecisionDocument>| RawIntake {
        docs,
        preview: None,
    };
    timed_line(
        OP,
        "intake_rgba16",
        "u16",
        &sizes,
        host,
        |input| intake(input).map(|doc| only(vec![doc])),
        raw_intake_bytes,
    );
    timed_line(
        OP,
        "intake_host_pixels",
        "u16",
        &sizes,
        |size| {
            let bytes: Vec<u8> = raw_intake_samples(size)
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
                .collect();
            (size, bytes)
        },
        |((w, h), bytes)| {
            let profile = IccProfile::srgb();
            let layout = std::ffi::CString::new("rgba16_le").unwrap();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_from_host_pixels(
                    bytes.as_ptr(),
                    bytes.len(),
                    *w,
                    *h,
                    *w as usize * 8,
                    layout.as_ptr(),
                    1,
                    profile.bytes().as_ptr(),
                    profile.bytes().len(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            plates_error(out.is_null(), error)?;
            Ok(only(vec![*unsafe { Box::from_raw(out) }]))
        },
        raw_intake_bytes,
    );
    timed_line(
        OP,
        "pipeline_exif6",
        "u16",
        &sizes,
        host,
        |input| {
            let doc = intake(input)?;
            let turned = doc
                .raw_source_geometry(
                    r#"{"version":1,"kind":"exif","orientation":6}"#,
                    REFERENCE_WORKING_BYTES,
                    0,
                    None,
                )?
                .ok_or_else(|| "no change".to_string())?;
            Ok(only(vec![doc, turned]))
        },
        raw_intake_bytes,
    );
    let lens = json!({"version":1,"kind":"lens","profile_id":"bench","center":[0.5,0.5],
        "radial":[0.02,0.0,0.0],"red_scale":1.0,"blue_scale":1.0,"filter":"bilinear"})
    .to_string();
    timed_line(
        OP,
        "pipeline_develop_lens",
        "u16",
        &sizes,
        host,
        |input| {
            let doc = intake(input)?;
            let (w, h) = doc.dimensions();
            let vignette = json!({"stops":-1.0,"center":[f64::from(w) / 2.0, f64::from(h) / 2.0],
                "falloff":1.0})
            .to_string();
            let developed = develop(
                &doc,
                &[],
                &[],
                "[]",
                Some(&vignette),
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )?
            .ok_or_else(|| "no change".to_string())?;
            let corrected = developed
                .raw_source_geometry(&lens, REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())?;
            Ok(only(vec![doc, developed, corrected]))
        },
        raw_intake_bytes,
    );
    timed_line(
        OP,
        "pipeline_preview_fit",
        "u16",
        &sizes,
        host,
        |input| {
            let doc = intake(input)?;
            let (w, h) = doc.dimensions();
            let preview = raw_preview(&doc, [0, 0, w, h], 4, 0)?;
            Ok(RawIntake {
                docs: vec![doc],
                preview: Some(preview),
            })
        },
        raw_intake_bytes,
    );
}

/// A temporary file, removed on drop.
struct DecoderFile(std::path::PathBuf);

impl DecoderFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("phase13b_decoders_{}_{name}", std::process::id())))
    }
    fn text(&self) -> &str {
        self.0.to_str().expect("a UTF-8 path")
    }
}

impl Drop for DecoderFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A `typed/decoders` answer: every intermediate the call made, alive until
/// dropped; the hashed answer is the preview, else the flattened RGBA8, else
/// the last image.
#[derive(Default)]
struct Decoded {
    _docs: Vec<PrecisionDocument>,
    images: Vec<PrecisionImage>,
    payload: Option<ClipboardPayload>,
    _narrowed: Option<DrDocument>,
    preview: Option<Box<DrImage>>,
    flat: Option<image::RgbaImage>,
}

fn decoded_bytes(answer: &Decoded) -> Vec<u8> {
    if let Some(preview) = &answer.preview {
        return raw_preview_bytes(preview);
    }
    if let Some(flat) = &answer.flat {
        return flat.as_raw().clone();
    }
    plates_image_bytes(answer.images.last().expect("a projection"))
}

fn decoders_native(doc: &PrecisionDocument) -> Result<Vec<u8>, String> {
    doc.encode_native(REFERENCE_WORKING_BYTES, 0, None)
}

fn decoders_parse(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, REFERENCE_WORKING_BYTES, 0, None)
}

fn decoders_with_empty(doc: &PrecisionDocument) -> Result<PrecisionDocument, String> {
    doc.add_layer_at("Empty", 0, InsertPosition::Top)?
        .ok_or_else(|| "no change".to_string())
}

/// `doc` rendered, pushed after `doc` itself.
fn decoders_rendered(doc: PrecisionDocument) -> Result<Decoded, String> {
    let projection = doc.render(REFERENCE_WORKING_BYTES, 0, None)?;
    Ok(Decoded {
        _docs: vec![doc],
        images: vec![projection],
        ..Decoded::default()
    })
}

/// `dr_precision_doc_source_master16` of the first layer carrying one.
fn decoders_master16(doc: &PrecisionDocument) -> Result<PrecisionImage, String> {
    let mut last = "no layer".to_string();
    for layer in 0..doc.layer_count() {
        let mut error = ptr::null_mut();
        let out = unsafe { dr_precision_doc_source_master16(doc, layer, &mut error) };
        match plates_error(out.is_null(), error) {
            Ok(()) => return Ok(*unsafe { Box::from_raw(out) }),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// `dr_precision_image_develop_preview` of the whole image, divisor 4,
/// clipping 0.
fn decoders_preview(image: &PrecisionImage) -> Result<Box<DrImage>, String> {
    let (w, h) = image.dimensions();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_develop_preview(
            image,
            0,
            0,
            w,
            h,
            4,
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(unsafe { Box::from_raw(out) })
}

/// `dr_precision_doc_open_psd` of `file` in `mode` (0 layered, 1
/// composite) against its import report's fingerprint.
fn decoders_open_psd(
    (file, fingerprint): &(DecoderFile, std::ffi::CString),
    mode: i32,
) -> Result<PrecisionDocument, String> {
    let path = std::ffi::CString::new(file.text()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_psd(
            path.as_ptr(),
            mode,
            fingerprint.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    plates_error(out.is_null(), error)?;
    Ok(*unsafe { Box::from_raw(out) })
}

/// The RZCB stream of layer 0's whole-canvas Copy of `doc`.
fn decoders_rzcb(doc: &PrecisionDocument) -> Vec<u8> {
    let (w, h) = doc.dimensions();
    let read = ClipboardRead::Copy {
        layer: 0,
        x: 0,
        y: 0,
        width: w,
        height: h,
    };
    doc.clipboard_copy(&read, None, REFERENCE_WORKING_BYTES, 0, None)
        .expect("a payload")
        .encode()
        .expect("an RZCB stream")
}

/// `payload` pasted in place into `target`, then rendered.
fn decoders_paste(
    target: &PrecisionDocument,
    payload: ClipboardPayload,
) -> Result<Decoded, String> {
    let request = PasteRequest {
        name: "Pasted".into(),
        anchor: 0,
        position: InsertPosition::Top,
        placement: PastePlacement::InPlace,
        profile: PasteProfile::Convert,
        frame: None,
    };
    let pasted = target.clipboard_paste(&payload, &request, REFERENCE_WORKING_BYTES, 0, None)?;
    let mut answer = decoders_rendered(pasted)?;
    answer.payload = Some(payload);
    Ok(answer)
}

/// The 13B-3e-4b lines (`DR_TILE_SHARING_ONLY=decoders`): `typed/decoders`.
fn typed_decoders() {
    const OP: &str = "typed/decoders";
    let sizes = [phase4_corpus::DIMENSIONS[0], FLOAT_DIMENSIONS];
    let floats = [FLOAT_DIMENSIONS];
    let base = |size| *unsafe { Box::from_raw(typed_base(size)) };
    let float = |size| *unsafe { Box::from_raw(float_corpus_document(size)) };
    let refused = |input: &Result<Vec<u8>, String>| input.clone();

    timed_line(
        OP,
        "reopen_rz16_render",
        "u16",
        &sizes,
        |size| {
            let warped = base(size)
                .warp_edit(&warp_request(4, 4), REFERENCE_WORKING_BYTES, 0, None)?
                .ok_or_else(|| "no change".to_string())?;
            decoders_native(&decoders_with_empty(&warped)?)
        },
        |input| decoders_rendered(decoders_parse(&refused(input)?)?),
        decoded_bytes,
    );
    timed_line(
        OP,
        "reopen_rz16_master_preview",
        "u16",
        &sizes,
        |size| {
            let host = base(size);
            let source = base(size);
            let request = std::ffi::CString::new(
                json!({"op":"place","source_id":"13b3e4b-bench","name":"Original","x":0,"y":0})
                    .to_string(),
            )
            .unwrap();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_source_edit(
                    &host,
                    &source,
                    request.as_ptr(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            plates_error(out.is_null(), error)?;
            let placed = *unsafe { Box::from_raw(out) };
            decoders_native(&placed)
        },
        |input| {
            let doc = decoders_parse(&refused(input)?)?;
            let master = decoders_master16(&doc)?;
            let preview = decoders_preview(&master)?;
            Ok(Decoded {
                _docs: vec![doc],
                images: vec![master],
                preview: Some(preview),
                ..Decoded::default()
            })
        },
        decoded_bytes,
    );
    timed_line(
        OP,
        "reopen_rzf_render",
        "f32",
        &floats,
        |size| decoders_native(&decoders_with_empty(&float(size))?),
        |input| decoders_rendered(decoders_parse(&refused(input)?)?),
        decoded_bytes,
    );
    timed_line(
        OP,
        "open_png16_preview",
        "u16",
        &sizes,
        |size| {
            let file = DecoderFile::new("open.png");
            let doc = base(size);
            precision_codec::save(
                doc.layer_source(0).expect("the corpus raster"),
                &Metadata::default(),
                Resolution::default(),
                file.text(),
                PrecisionFormat::Png,
                true,
                false,
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .expect("the PNG16 written");
            file
        },
        |file| {
            let doc =
                PrecisionDocument::open_preserving(file.text(), REFERENCE_WORKING_BYTES, 0, None)?;
            let preview = decoders_preview(doc.layer_source(0).expect("a raster"))?;
            Ok(Decoded {
                _docs: vec![doc],
                preview: Some(preview),
                ..Decoded::default()
            })
        },
        decoded_bytes,
    );
    let psd = |size| {
        let doc = decoders_with_empty(&base(size)).expect("an added layer");
        let projection = doc
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .expect("the projection");
        let file = DecoderFile::new("open.psd");
        let path = std::ffi::CString::new(file.text()).unwrap();
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_psd(
                &doc,
                &projection,
                path.as_ptr(),
                0,
                0,
                REFERENCE_WORKING_BYTES,
                0,
                std::ptr::null(),
                &mut error,
            )
        };
        plates_error(!ok, error).expect("the layered PSD written");
        let mut error = ptr::null_mut();
        let report = unsafe { dr_psd_import_report(path.as_ptr(), &mut error) };
        let text = whole_text(report, error).expect("the PSD import report");
        let report: Value = serde_json::from_slice(&text).expect("report JSON");
        let fingerprint = report["source_fingerprint"]
            .as_str()
            .expect("a fingerprint");
        (file, std::ffi::CString::new(fingerprint).unwrap())
    };
    for (name, mode) in [("open_psd16_render", 0), ("open_psd16_composite", 1)] {
        timed_line(
            OP,
            name,
            "u16",
            &sizes,
            psd,
            |input| decoders_rendered(decoders_open_psd(input, mode)?),
            decoded_bytes,
        );
    }
    timed_line(
        OP,
        "open_tiff32_render",
        "f32",
        &floats,
        |(width, height)| {
            let samples: Vec<f32> = phase4_corpus::image(width, height)
                .into_raw()
                .into_iter()
                .map(|v| f32::from(v) / 255.0)
                .collect();
            let tiff = FloatTiff {
                width,
                height,
                channels: 4,
                alpha: Some(false),
                samples,
                icc: None,
                resolution: None,
                metadata: Metadata::default(),
                non_finite: 0,
            };
            let file = DecoderFile::new("open32.tiff");
            let encoded =
                doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("a float TIFF");
            std::fs::write(&file.0, encoded).expect("the float TIFF written");
            file
        },
        |file| {
            decoders_rendered(PrecisionDocument::open_preserving(
                file.text(),
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )?)
        },
        decoded_bytes,
    );
    timed_line(
        OP,
        "paste_rzcb16",
        "u16",
        &sizes,
        |size| (decoders_rzcb(&base(size)), base(size)),
        |(stream, target)| {
            decoders_paste(
                target,
                ClipboardPayload::decode(stream, REFERENCE_WORKING_BYTES, 0)?,
            )
        },
        decoded_bytes,
    );
    timed_line(
        OP,
        "paste_rzcbf",
        "f32",
        &floats,
        |size| (decoders_rzcb(&float(size)), float(size)),
        |(stream, target)| {
            decoders_paste(
                target,
                ClipboardPayload::decode(stream, REFERENCE_WORKING_BYTES, 0)?,
            )
        },
        decoded_bytes,
    );
    timed_line(
        OP,
        "paste_rgba8",
        "u16",
        &sizes,
        |size| (phase4_corpus::image(size.0, size.1).into_raw(), base(size)),
        |(pixels, target)| {
            let (w, h) = target.dimensions();
            let payload = ClipboardPayload::from_rgba8(
                pixels,
                w,
                h,
                IccProfile::srgb().bytes(),
                (0, 0),
                Resolution::default(),
                REFERENCE_WORKING_BYTES,
                0,
            )?;
            decoders_paste(target, payload)
        },
        decoded_bytes,
    );
    let invert =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    timed_line(
        OP,
        "narrow_render",
        "u16",
        &sizes,
        |size| {
            let doc = base(size)
                .add_adjustment("Invert", &invert, 1.0, REFERENCE_WORKING_BYTES, 0, None)
                .expect("an adjustment")
                .expect("a changed document");
            let mask = MaskEdit::parse(
                &json!({"op":"set_mask","layer":1,"width":size.0,"height":size.1,"x":0,"y":0})
                    .to_string(),
            )
            .expect("a mask request");
            doc.mask_edit(
                &mask,
                &producer_coverage(size),
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .expect("a mask")
            .expect("a masked document")
        },
        |doc| {
            let narrowed = doc.narrow_structure_to_u8(
                PrecisionDither::None,
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )?;
            let flat = narrowed.flattened()?;
            Ok(Decoded {
                _narrowed: Some(narrowed),
                flat: Some(flat),
                ..Decoded::default()
            })
        },
        decoded_bytes,
    );
}
