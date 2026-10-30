#!/usr/bin/env python3
"""The #1 CMYK admission drive, on a built Release app.

Every new CMYK plan is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so a document's
window and display projection are inside the interval). The gates are FIXED
here before the drive runs; a miss is recorded in `receipt.json` and fails, and
the answer to a miss is the plan's existing margin term, never this bound.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. Fixtures are
6000x4000 (24 MP): a 16-bit and an 8-bit chunky CMYK TIFF written HERE (a
horizontal ink gradient with per-column noise, so no tile is uniform and none
is shared) carrying macOS's Generic CMYK Profile, a CMYK JPEG and a CMYK PSD
the app writes from them in a setup app, and a 16-bit RGB PNG
(`scripts/host-still-admission/author.swift png16`).

The plans (the core's own figures, `get_precision`'s `plans` for a CMYK
document, and `convert_mode`'s reply):

- `open-tiff16`, `open-tiff8`, `open-jpeg8`, `open-psd16`: `open_document`;
  plan = pixels x `cmyk_open_bytes_per_pixel` + the file's bytes + 1 MiB
  (`cmyk_io::open_scratch`).
- `to-cmyk-16`: an open 16-bit RGB document; `convert_mode cmyk`; plan = the
  reply's `working_memory_plan_bytes`.
- `to-rgb-16`: an open 16-bit CMYK document; `convert_mode rgb`; the same.
- `to-cmyk-8`: the 16-bit RGB PNG opened as an 8-bit document (`precision`
  rgba8); `convert_mode cmyk`, which promotes it and converts at depth 8; plan =
  the reply's `working_memory_plan_bytes` (`CmykModeConversion.scratch`'s
  8-bit term).
- `to-rgb-8`: an open 8-bit CMYK document; `convert_mode rgb`, which converts
  to 16 bits and narrows to 8; the same.
- `render-16`: `render` of the whole canvas as a region at divisor 1, which
  renders the view and then converts that output for display, the two stages
  of the view's own charge; plan = `projection_scratch_bytes`
  (`projectionScratchEstimate`: max(render scratch, render output + display
  scratch) + overhead, with `ProjectionMargin`).
- `flatten-16`: a two-layer CMYK document (`duplicate_layer`); `flatten_image`;
  plan = `cmyk_flatten_scratch_bytes`.
- `save-tiff16`: `save_copy tiff`; plan = `cmyk_save_scratch_bytes`.
- `save-psd16`: `export_psd flattened`; plan = `cmyk_psd_save_scratch_bytes`.
- `save-jpeg`: `save_copy jpeg`; plan = max(`cmyk_save_scratch_bytes`, the
  transcode's 8 x pixels + the intermediate's 4 x pixels, profile and 1 MiB)
  (`CapturedSave`: the core save ends before the ImageIO transcode starts).
- `native-save-16`: `save_copy rz`; plan = `native_scratch_bytes`.
- `native-reopen-16`: `open_document` of that `.rz`; plan =
  `decode_scratch_bytes` (the reopen's scratch beside `reopen_retained_bytes`,
  which the measured interval also holds, so the plan is their sum).

Phase 2 (#1): the edit plans `get_precision` reports for a CMYK document,
read BEFORE the measured call:

- `fill-16`: `select_all`, then `fill_selection` with `ink`; plan =
  `cmyk_paint_scratch_bytes` (a canvas-wide `paint_blend` on the top raster).
- `gradient-16`: `gradient` with `start_ink`/`end_ink` across the canvas; plan
  = `cmyk_gradient_scratch_bytes`.
- `bucket-16`: `fill` (the Paint Bucket) with `ink` on a UNIFORM 16-bit CMYK
  TIFF, so the flood covers the canvas; plan = `cmyk_bucket_scratch_bytes`.
- `adjust-apply-16`: `apply_filter curves` on the layer's ink; plan =
  `cmyk_adjust_apply_scratch_bytes`.
- `adjust-add-16`: `add_adjustment_layer curves`; plan =
  `cmyk_adjust_add_scratch_bytes`.
- `render-masked-16`: a Curves layer with a feathered (radius 20) mask, then
  `render` as `render-16`; plan = `projection_scratch_bytes`.
- `new-layer-16`: `new_layer`; plan = `cmyk_add_layer_scratch_bytes`.
- `huesat-apply-16`: `apply_filter hue_saturation` {hue: 40} on the layer's
  ink (the RGB kernel on 1 − C, 1 − M, 1 − Y with K kept; no memo); plan =
  `cmyk_huesat_apply_scratch_bytes`.
- `huesat-add-16`: `add_adjustment_layer hue_saturation` {hue: 40}; plan =
  `cmyk_huesat_add_scratch_bytes`.
- `render-huesat-16`: a {hue: 40} Hue/Saturation layer, then `render` as
  `render-16`; plan = `projection_scratch_bytes` (no memo term since the K
  rule).
- `native-save-83`: the Curves-and-mask document (word 83); `save_copy rz`;
  plan = `native_scratch_bytes`.
- `native-reopen-83`: `open_document` of that `.rz`; plan =
  `decode_scratch_bytes` + `reopen_retained_bytes`.

The selection batch (#1): the plans `get_precision` reports for the
selections, Stroke…, Apply Layer Mask and Select and Mask's New Layer, read
BEFORE the measured call (the measured call is the whole MCP tool, so its
interval also holds the host's commit of the selection or the edit):

- `wand-16`: `select_magic_wand` at (10, 10), tolerance 32; plan =
  `cmyk_wand_scratch_bytes` (the composite's ink canvas, the flood).
- `range-16`: `select_color_range` sampled, one 16-bit sample, tolerance 20;
  plan = `cmyk_range_scratch_bytes` (the view canvas and the link's nodes).
- `quick-16`: `quick_select` one seed, brush_size 201, smooth 1; plan =
  `cmyk_quick_scratch_bytes`.
- `refine-16`: a `select_rect`, then `refine_selection` edge_radius 8,
  smart_radius, output selection; plan = `cmyk_refine_scratch_bytes`.
- `stroke-edge-16`: `stroke_selection` with nothing selected on the opaque
  layer, width 10, outside, `ink`; plan = `cmyk_stroke_edge_scratch_bytes`
  (the layer grows past the canvas by the band; the descriptor's figure is
  the bound over the layer's stored extent, equal to the edit's own plan on
  this opaque layer).
- `apply-mask-16`: a from-selection mask on the layer (its left half), then
  `remove_layer_mask` apply; plan = `cmyk_apply_mask_scratch_bytes`.
- `cutout-16`: a `select_rect`, then `refine_selection` feather 8, output
  new_layer_with_mask; plan = `cmyk_cutout_scratch_bytes`.
- `cutout-decontam-16`: the same with decontaminate; plan =
  `cmyk_cutout_decontaminate_scratch_bytes`.
- `bucket-all-16`: `fill` with sample_all_layers on the UNIFORM 16-bit TIFF;
  gated at max(`cmyk_wand_scratch_bytes`, `cmyk_paint_scratch_bytes`), the
  region and then the paint, which never run together.

Phase 4a (#1): the structure plans, `get_precision`'s `plans` read BEFORE
the measured call:

- `merge-16`: a two-layer CMYK document (`duplicate_layer`); `merge_visible`;
  plan = `cmyk_merge_scratch_bytes` (`cmyk_merge`).
- `stamp-16`: the same document; `stamp_visible`; plan =
  `cmyk_stamp_scratch_bytes`.
- `render-structure-16`: two layers in a Pass Through group with a feathered
  (radius 20) mask, the top one Multiply, and a filled layer clipped above the
  group; `render` as `render-16`; plan = `projection_scratch_bytes` (the
  stack's group and clip row buffers).
- `native-save-84`: that document (word 84); `save_copy rz`; plan =
  `native_scratch_bytes`.
- `native-reopen-84`: `open_document` of that `.rz`; plan =
  `decode_scratch_bytes` + `reopen_retained_bytes`.

Phase 5 (#1): the colour workflow, each plan the core's own figure for the
call (a reply's `working_memory_plan_bytes`, or `get_precision`'s `plans`):

- `cmyk-convert-16`: `convert_profile` (profile installed) into a second
  installed press (Coated FOGRA39 when present, else the first other proofable
  CMYK profile); plan = the reply's `working_memory_plan_bytes`
  (`cmyk_profile`'s convert terms).
- `cmyk-assign-16`: `assign_profile` to the same press; the same.
- `cmyk-depth-8`: `convert_precision cmyk8`; the same.
- `new-cmyk-16`: `new_document` color_mode cmyk, 6000x4000 px, depth 16,
  white; plan = the new document's `cmyk_new_scratch_bytes`.
- `proof-cmyk-16`: `render_proof` through `working_cmyk` (max_side 64); plan =
  the reply's `working_memory_plan_bytes` (`cmyk_proof`).
- `tone-analysis-16`: `histogram` (the composite's per-ink histograms); plan =
  the reply's `working_memory_plan_bytes` (`cmyk_tone_analysis`).
- `clip-display-16`: `render` with `clipping` shadows (max_side 64); plan = the
  reply's `working_memory_plan_bytes` (`cmyk_clip_display`).
- `export-cmyk-png`: `export_image` png to sRGB; plan = the reply's
  `working_memory_plan_bytes` (`cmyk_export`'s conversion and stages).
- `export-cmyk-tiff-keep`: `export_image` tiff, bit_depth 16, profile
  document (kept CMYK); the same.

Round 2 (#1): the kept-CMYK export's stages (`cmyk_export_ink`), each
`export_image` with profile document; plan = the reply's
`working_memory_plan_bytes`:

- `export-cmyk-keep-resize-16`: tiff, bit_depth 16, size scale 1.5, lanczos3.
- `export-cmyk-keep-sharpen-16`: tiff, bit_depth 16, Unsharp Mask radius 2,
  amount 1.
- `export-cmyk-keep-depth-8`: the 16-bit fixture as tiff, bit_depth 8.
- `export-cmyk-keep-depth-16`: the 8-bit fixture as tiff, bit_depth 16.
- `export-cmyk-jpeg-keep-staged`: jpeg, size scale 1.5, Unsharp Mask radius 2,
  amount 1, and a metadata policy with a copyright stamp.

Phase 4b (#1): geometry on ink. The first six are gated on the descriptor's
geometry plans (`cmyk_geometry_plans`), read BEFORE the measured call, each the
core plan of the very request the drive sends (w × h the canvas):

- `resize-16`: `image_size` to ⌈3w/2⌉ × ⌈3h/2⌉, catmull-rom (bicubic); plan =
  `cmyk_resize_scratch_bytes`.
- `affine-16`: `transform_layer` the layer 17° about its centre (the canvas
  centre), bilinear; plan = `cmyk_affine_scratch_bytes`.
- `perspective-crop-16`: `perspective_crop` of the quad inset 10 % with a 5 %
  keystone ((0.15w, 0.1h), (0.85w, 0.1h), (0.9w, 0.9h), (0.1w, 0.9h)) onto
  w × h, bilinear; plan = `cmyk_perspective_crop_scratch_bytes`.
- `warp-16`: `warp_layer set` 4 × 4 bicubic with node (1, 1) moved (60, 40);
  plan = `cmyk_warp_scratch_bytes`.
- `liquify-16`: `liquify` one Forward Warp stroke, size 500, from the canvas
  centre 300 px right; plan = `cmyk_liquify_scratch_bytes`.
- `transform-selected-16`: `select_all`, then `transform_selected_pixels` 7°
  bilinear (the host sends the selection's bounds, the canvas); plan =
  `cmyk_transform_selected_scratch_bytes`.

The two mesh warps are gated on the plan the host admits, by arithmetic from
the core's formulas as `scripts/puppet-warp-admission` and
`scripts/perspective-warp-admission` state them (n = 24,000,000, D the warped
layer's area read back from `get_document`, M = `MARGIN_BYTES_PER_PIXEL` = 4,
T = D's 256-px tiles): `precision_puppet::mesh_warp_plan`'s CMYK arm charges
the CMYK source copy (10n), the destination at 10 B/px with its margin
(D(10 + M)) and the adoption copy (10D + 64T), and the host adds the 24n
projection (`MeshWarpWork.prepareLayer`):

- `puppet-16`: `puppet_warp` with the four corner pins (300, 300), (5700,
  300), (300, 3700), (5700, 3700) held and (3000, 2000) moved to (3060, 2040);
  P = COMMON + 10n + D(10 + M) + 10D + 64T + 24n, COMMON = MESH(6000, 4000) +
  `SOLVER_BYTES` + `MAP_BYTES` + 2 MiB (`doc_puppet::common_scratch`).
- `perspective-warp-16`: `perspective_warp` with the one keystone quad
  (300, 300)…(300, 3700) → (500, 300), (5500, 300), (5800, 3700), (200, 3700);
  P = WARP + 10n + D(10 + M) + 10D + 64T + 24n, WARP = `GRID_BYTES` +
  `SOLVER_BYTES` + `MAP_BYTES` + 2 × `TRIANGLE_BYTES` + 2 MiB.

Trim reuses `cmyk_merge_scratch_bytes` (measured as `stamp-16`).

The adjustments and filters (#1), `get_precision`'s `plans` read BEFORE the
measured call:

- `filter-gaussian-16`: `apply_filter blur` sigma 8 (Filter ▸ Blur ▸ Gaussian
  Blur's op) on the layer's ink; plan = `cmyk_filter_gaussian_scratch_bytes`
  (`cmyk_filter`: the RGB pass's own plan, the two `L` passes and the ink).
- `filter-surface-16`: `apply_filter surface_blur` radius 10, threshold 15;
  plan = `cmyk_filter_surface_scratch_bytes`.
- `filter-dust-16`: `apply_filter dust_and_scratches` radius 4, threshold 0;
  plan = `cmyk_filter_dust_scratch_bytes`.
- `shadows-apply-16`: `apply_filter shadows_highlights` at its defaults (the
  ink-tone guide); plan = `cmyk_shadows_apply_scratch_bytes`.
- `fade-16`: an `apply_filter blur` sigma 8 is the setup; `fade` 50 %; plan =
  `cmyk_fade_scratch_bytes` (`cmyk_fade`).

Phase 4c (#1): copy and paste, Layer Via, Stamp Selected and Merge Group,
the plans `get_precision` reports, read BEFORE the measured call. Every case
runs on its OWN private pasteboard (`DR_PASTEBOARD_NAME`, set per app before it
starts), so nothing reaches the general pasteboard. A second CMYK profile is
Adobe's U.S. Web Coated (SWOP) v2 (`SWOP`; absent, its two cases record the
refusal and fail):

- `copy-16`, `copy-merged-16`, `cut-16`: `select_all` on the 16-bit TIFF, then
  `copy` layer 0 / `copy_merged` / `cut` layer 0; plan = `cmyk_copy_scratch_bytes`
  / `cmyk_copy_merged_scratch_bytes` / `cmyk_cut_scratch_bytes`.
- `paste-16`: the TIFF copied whole in the setup; `paste` in place into a
  second copy of it; plan = the target's `cmyk_paste_scratch_bytes`.
- `paste-from-rgb-16`: the 16-bit RGB PNG copied whole; `paste` in place into
  the TIFF; plan = the target's `cmyk_paste_convert_scratch_bytes`.
- `paste-profile-16`: a SWOP TIFF copied whole; `paste` in place into the
  Generic TIFF; plan = the target's `cmyk_paste_convert_scratch_bytes`.
- `paste-to-rgb-16`: the TIFF copied whole; `paste` in place into the RGB PNG
  document; plan = the SOURCE's `cmyk_paste_to_rgb_scratch_bytes`.
- `layer-via-copy-16`, `layer-via-cut-16`: `select_all`, then `layer_via_copy`
  / `layer_via_cut` layer 0; plan = `cmyk_layer_via_copy_scratch_bytes` /
  `cmyk_layer_via_cut_scratch_bytes`.
- `paste-layers-profile-16`: the SWOP TIFF's layer copied as layers; `paste`
  in place into the Generic TIFF; plan = the target's
  `cmyk_paste_layers_convert_scratch_bytes`.
- `stamp-selected-16`: a two-layer document; `stamp_selected` of both; plan =
  `cmyk_stamp_selected_scratch_bytes`.
- `merge-group-16`: the two layers grouped; `merge_group`; plan =
  `cmyk_merge_group_scratch_bytes`.

Phase 3 (#1): layered CMYK files, each plan the core's own figure read
BEFORE the measured call:

- `export-psd-layers-16`: the `render-structure-16` document with its group
  mask's feather cleared (a PSD mask stores none, so the export refuses one); `export_psd`
  layers, PSD; plan = `inspect_psd_export`'s `working_memory_plan_bytes` for
  the same request (`psd_export_cmyk`'s plan, `LAYERED_SAVE_BYTES_PER_PIXEL`).
- `open-psd-layers-16`: that export (written in the setup app) in a fresh
  instance; `open_document` psd_mode layers; plan = `inspect_psd`'s
  `layered_plan_bytes` (`psd_import_cmyk::layered_plan`,
  `LAYERED_OPEN_BYTES_PER_PIXEL`).
- `open-tiff-alpha-16`: a 16-bit chunky CMYKA TIFF written HERE (the
  gradient, an unassociated alpha ramp); `open_document`; plan = the open's
  `cmyk_io::open_scratch`: pixels x `cmyk_open_bytes_per_pixel` + the file's
  bytes + the alpha's decoded bytes (2 x pixels) + 1 MiB.

The channels batch (#1): the selection kinds, the DRDC 85 document and one
ink plate's selected pixels, `get_precision`'s `plans` read BEFORE the
measured call:

- `plate-load-16`: `load_selection` from plane cyan (the plate's 1 − ink);
  plan = `cmyk_plate_load_scratch_bytes`.
- `luminance-16`: `load_selection` from plane luma (the composite's sRGB
  luma); plan = `cmyk_luminance_scratch_bytes`.
- `similar-16`: the left-half rectangle, then `select_similar` tolerance 32,
  global; plan = `cmyk_similar_scratch_bytes`.
- `native-save-85`: the 16-bit TIFF with two alpha channels (the left half,
  and the right half) and a rectangular vector mask on its layer (word 85);
  `save_copy rz`; plan = `native_scratch_bytes`.
- `native-reopen-85`: `open_document` of that `.rz`; plan =
  `decode_scratch_bytes` + `reopen_retained_bytes`.
- `move-plate-16`: `select_all`, then `move_selected_pixels` target cyan dx 37
  dy 23 background #CCCCCC; plan = `cmyk_move_plate_scratch_bytes`.
- `transform-plate-16`: `select_all`, then `transform_selected_pixels` target
  cyan 7° bilinear background #CCCCCC; plan = `cmyk_transform_plate_scratch_bytes`.

No MCP tool or menu issues a backdrop selection (the core's `source`
`backdrop` is reached only through `dr_precision_doc_select`), so the
descriptor's `cmyk_backdrop_wand_scratch_bytes` — the composite wand's
formula with the whole document's stack, an upper bound of the backdrop's —
has no case here; `wand-16` is its measured record.

The retouch batch (#1), `get_precision`'s `plans` read BEFORE the measured
call, each the core plan of the very request the drive sends (the descriptor's
`cmyk_retouch_plate::cmyk_retouch_plans`; the canvas centre is (3000, 2000)):

- `retouch-clone-16`: `clone_stamp` sample current along the centre row from
  x 1500 to 4500, size 100, source 400 px above; plan =
  `cmyk_clone_scratch_bytes` (the clone's plan reads no stroke geometry).
- `retouch-heal-16`: `heal_stroke` one 100 px dab at the centre, source 400 px
  above; plan = `cmyk_heal_scratch_bytes` (bounds the 104 px square there,
  `NativeBrushStroke.healBounds`).
- `spot-heal-16`: `spot_heal_stroke` one 100 px dab at the centre; plan =
  `cmyk_spot_heal_scratch_bytes`.
- `dodge-16`: `dodge_burn` exposure 50 midtones along the centre row; plan =
  `cmyk_dodge_scratch_bytes`.
- `smudge-16`: `smudge_stroke` from (1500, 2000) to (4500, 2000), size 100,
  strength 50; plan = `cmyk_smudge_scratch_bytes`.
- `history-brush-16`: a Gaussian Blur 8 as the setup, then
  `history_brush_stroke` from the state before it along the centre row, size
  100; plan = `cmyk_history_brush_scratch_bytes`.
- `equalize-16`: `equalize` on the layer; plan = `cmyk_equalize_scratch_bytes`.
- `blur-sal-rgb16`, `blur-sal-16`: the 16-bit RGB PNG at 16 bits, and the
  16-bit CMYK TIFF; `blur_stroke` with `sample_all_layers` along the diagonal
  (100, 100)–(5900, 3900), size 200, strength 50, so the walk rect is nearly the
  whole layer; plan = the reply's `working_memory_plan_bytes` (the stroke job's
  admitted plan: the footprint's and the edit's, which adds the sampled picture,
  `sampled_plate_plan`, to the walk's term once).
- `smudge-sal-rgb16`, `smudge-sal-16`: the same documents; `smudge_stroke`
  with `sample_all_layers` along the same diagonal at size 20, strength 50 (a
  5820 × 3820 footprint inside Smudge's work bound); plan = the same reply
  figure.
- `art-history-8`: the 8-bit CMYK TIFF, a Gaussian Blur 8 as the setup
  (as `history-brush-16`'s), then `art_history_brush_stroke` from the opened
  state (the History panel's source) along the diagonal (100, 100)–(5900,
  3900), loose_long, Area 500, size 100, so the walk rect is the whole layer;
  plan = the reply's `working_memory_plan_bytes` (the stroke job's plan,
  `dr_precision_doc_art_history_brush_plan`). The tool refuses at 16 bits
  (`art_history_take.py` take1), so 8-bit CMYK is the only typed document
  it runs on and the only typed plan there is to admit.
- `equalize-rgb16`, `equalize-rgb32`: the 16-bit RGB PNG opened at 16 bits (and
  converted to 32-bit float, structure preserve), then `equalize`; plan by
  arithmetic from `equalize_apply::equalize_plan` (n = the canvas = the layer,
  B = 1 MiB bookkeeping, the table 65536 × 8): 16-bit `6B + 8n + 4·65536·8 +
  8n + 2n + 65536·8`; float `6B + 40n + (2n + ⌈2n/4⌉) + 65536·8`.
  The 16-bit row is RE-DECLARED after its first run (`admission-2` in the
  retouch batch's handoff: E 768.9 MB against the edit plan alone, need
  961.1 MB). Its E is the whole call, the edit plus the 16-bit display
  preparation `PreparedDocumentEdit` admits after it, so it is gated as
  gaussian-blur-admission's P16 and `ProjectionMargin`'s H2 (the same 24 MP
  16-bit invert, E 765 MB, "the edit's own plan 442,500,718 is below"):
  plan = max(edit plan, `PROJECTION_16`) + dR, dR the smallest growth of
  `unique_retained_bytes` across the call. The float row keeps its edit plan.

The artwork batch (#1): type, shape and fill layers in ink, each gated on
the plan the host admits for the render (`NativeArtworkEvaluation.scratchBytes`
+ `cmykMarginPerPixel` × the raster + 64 × the profile), which the reply
echoes as `working_memory_plan_bytes`:

- `artwork-fill-16`: `add_fill_layer` solid, `ink` [20, 40, 60, 10], over the
  6000 × 4000 canvas.
- `artwork-shape-16`: `add_shape_layer` rect (0, 0, 6000, 4000) with
  `fill_ink` and an 8 px `stroke_ink` (a raster the stroke and padding make a
  few pixels larger than the canvas, two coverage planes).
- `artwork-edit-16`: a solid fill layer as the setup, then `edit_fill_layer`
  recolouring it (`ink` [60, 0, 20, 40]).

The styles batch (#1): layer effects and knockout on ink, each plan read
from `get_precision` BEFORE the measured call:

- `render-styles-16`: the layer duplicated and styled (`STYLE`: a Multiply
  drop shadow, an outer glow, an outside stroke and a colour overlay, in
  ink); `render`; plan = `projection_scratch_bytes` (the stack's
  `cmyk_style::styled_scratch` included).
- `merge-styles-16`: the same document; `merge_visible`; plan =
  `cmyk_merge_scratch_bytes`.
- `render-styled-group-16`: the two layers grouped (isolated) and the GROUP
  styled; `render`; plan = `projection_scratch_bytes` (the group's ink frame
  included).
- `render-stacked-16`: the layer duplicated and styled with
  `stacked_style` — THREE instances of each repeating kind (drop shadow,
  inner shadow, outside stroke, colour overlay, gradient overlay) in ink;
  `render`; plan = `projection_scratch_bytes`.
- `render-stacked-rgb16`: the same on the 16-bit RGB PNG, in sRGB
  colours; `render`; plan = `projection_scratch_bytes`.
- `merge-knockout-16`: the layer duplicated twice, the top one a deep
  knockout at Fill 0 over the Background; `merge_down` of it; plan =
  `cmyk_merge_down_scratch_bytes` (the members' plan beside the seed's
  stack, `cmyk_merge::cmyk_seed_scratch_bytes`).

The remaining-gaps batch (#1): one ink plate as a scalar target, Apply Image on
CMYK, Gamut Warning and the reduced region. Each plan is the one the host admits
the call at, which the reply reports as `working_memory_plan_bytes` (no
descriptor key holds it): the core's scalar plan (`ScalarEdit::plan`, its plate
terms n·2·10 + canvas·`DISPLAY_BYTES_PER_PIXEL`) for the two plate edits, and
`NativeChannelMath.scratch` (the largest core stage, 128 B per canvas pixel of
resident planes and the display preparation) for Apply Image:

- `plate-fill-16`: `select_all`, then `fill` target cyan fill_mode selection
  color #000000 at (10, 10) (the scalar `fill` on the plate's 1 − ink; the
  tool requires the seed, which the selection mode does not read).
- `plate-gaussian-16`: `apply_filter blur` sigma 8 target cyan.
- `apply-image-cmyk-16`: `apply_image` target layer source_plane cmyk blend
  Multiply (the four plates, one `replace_canvas` each).
- `proof-gamut-16`: `render_proof` through the second press (as
  `cmyk-convert-16` picks it) with gamut_warning true (max_side 64); plan = the
  reply's `working_memory_plan_bytes` (`cmyk_proof`, its memo of
  `([u8; 3], bool)`).
- `render-divisor-16`: `render` of the whole canvas at preview_divisor 2
  (max_side 64); plan = `projection_scratch_bytes`, as `render-16` (the
  divisor-2 region plan is a quarter of the divisor-1 one, so the projection's
  charge bounds it).

A row whose max E is not above its baseline twins' prints `SKIP (no rise)` and
does not count; the gate FAILS when every selected row was skipped.

Usage: python3 -B scripts/cmyk-admission/measure.py --app <Doctored.app>
[--output <folder outside the repository>] [--runs 3] [--cases a,b]. Exit 0
only when the gate passes.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
AUTHOR = ROOT / 'scripts/host-still-admission/author.swift'
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
from verify import call, footprint  # noqa: E402
import support  # noqa: E402

LIMIT = 8589934592
SIZE = (6000, 4000)
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SWOP = Path('/Library/Application Support/Adobe/Color/Profiles/Recommended/USWebCoatedSWOP.icc')
MIB = 1 << 20

# name: (the setup's fixture, the measured call)
CASES = {
    'open-tiff16': ('none', 'open:tiff16'),
    'open-tiff8': ('none', 'open:tiff8'),
    'open-jpeg8': ('none', 'open:jpeg8'),
    'open-psd16': ('none', 'open:psd16'),
    'to-cmyk-16': ('png16', 'to-cmyk'),
    'to-rgb-16': ('tiff16', 'to-rgb'),
    'to-cmyk-8': ('png8', 'to-cmyk'),
    'to-rgb-8': ('tiff8', 'to-rgb'),
    'render-16': ('tiff16', 'render'),
    'flatten-16': ('tiff16-two', 'flatten'),
    'save-tiff16': ('tiff16', 'save:tiff'),
    'save-psd16': ('tiff16', 'save:psd'),
    'save-jpeg': ('tiff16', 'save:jpeg'),
    'native-save-16': ('tiff16', 'save:rz'),
    'native-reopen-16': ('none', 'open:rz16'),
    # Phase 2 (#1): the edit plans.
    'fill-16': ('tiff16', 'edit:fill'),
    'gradient-16': ('tiff16', 'edit:gradient'),
    'bucket-16': ('flat16', 'edit:bucket'),
    'adjust-apply-16': ('tiff16', 'edit:adjust-apply'),
    'adjust-add-16': ('tiff16', 'edit:adjust-add'),
    'render-masked-16': ('tiff16-masked', 'render'),
    'new-layer-16': ('tiff16', 'edit:new-layer'),
    'native-save-83': ('tiff16-masked', 'save:rz'),
    'native-reopen-83': ('none', 'open:rz83'),
    # #1: Hue/Saturation on ink, K held (no memo).
    'huesat-apply-16': ('tiff16', 'edit:huesat-apply'),
    'huesat-add-16': ('tiff16', 'edit:huesat-add'),
    'render-huesat-16': ('tiff16-huesat', 'render'),
    # The selection batch (#1).
    'wand-16': ('tiff16', 'edit:wand'),
    'range-16': ('tiff16', 'edit:range'),
    'quick-16': ('tiff16', 'edit:quick'),
    'refine-16': ('tiff16-selected', 'edit:refine'),
    'stroke-edge-16': ('tiff16', 'edit:stroke-edge'),
    'apply-mask-16': ('tiff16-layer-masked', 'edit:apply-mask'),
    'cutout-16': ('tiff16-selected', 'edit:cutout'),
    'cutout-decontam-16': ('tiff16-selected', 'edit:cutout-decontam'),
    'bucket-all-16': ('flat16', 'edit:bucket-all'),
    # Phase 4a (#1): merges, Stamp Visible and the structured document.
    'merge-16': ('tiff16-two', 'edit:merge-visible'),
    'stamp-16': ('tiff16-two', 'edit:stamp-visible'),
    'render-structure-16': ('tiff16-structure', 'render'),
    'native-save-84': ('tiff16-structure', 'save:rz'),
    'native-reopen-84': ('none', 'open:rz84'),
    # Phase 5 (#1): the colour workflow.
    'cmyk-convert-16': ('tiff16', 'colour:convert'),
    'cmyk-assign-16': ('tiff16', 'colour:assign'),
    'cmyk-depth-8': ('tiff16', 'colour:depth'),
    'new-cmyk-16': ('none', 'colour:new'),
    'proof-cmyk-16': ('tiff16', 'colour:proof'),
    'tone-analysis-16': ('tiff16', 'colour:histogram'),
    'clip-display-16': ('tiff16', 'colour:clip'),
    'export-cmyk-png': ('tiff16', 'colour:export-png'),
    'export-cmyk-tiff-keep': ('tiff16', 'colour:export-tiff'),
    # Phase 4b (#1): geometry on ink.
    'resize-16': ('tiff16', 'edit:resize'),
    'affine-16': ('tiff16', 'edit:affine'),
    'perspective-crop-16': ('tiff16', 'edit:perspective-crop'),
    'warp-16': ('tiff16', 'edit:warp'),
    'liquify-16': ('tiff16', 'edit:liquify'),
    'transform-selected-16': ('tiff16', 'edit:transform-selected'),
    'puppet-16': ('tiff16', 'mesh:puppet'),
    'perspective-warp-16': ('tiff16', 'mesh:perspective'),
    # #1: the adjustments and filters.
    'filter-gaussian-16': ('tiff16', 'edit:filter-gaussian'),
    'filter-surface-16': ('tiff16', 'edit:filter-surface'),
    'filter-dust-16': ('tiff16', 'edit:filter-dust'),
    'shadows-apply-16': ('tiff16', 'edit:shadows-apply'),
    'fade-16': ('tiff16-blurred', 'edit:fade'),
    # Phase 4c (#1): copy and paste, Layer Via, Stamp Selected, Merge Group.
    'copy-16': ('tiff16', 'clip:copy'),
    'copy-merged-16': ('tiff16', 'clip:copy-merged'),
    'cut-16': ('tiff16', 'clip:cut'),
    'paste-16': ('tiff16', 'clip:paste'),
    'paste-from-rgb-16': ('tiff16', 'clip:paste-from-rgb'),
    'paste-profile-16': ('tiff16', 'clip:paste-profile'),
    'paste-to-rgb-16': ('png16', 'clip:paste-to-rgb'),
    'layer-via-copy-16': ('tiff16', 'clip:layer-via-copy'),
    'layer-via-cut-16': ('tiff16', 'clip:layer-via-cut'),
    'paste-layers-profile-16': ('tiff16', 'clip:paste-layers-profile'),
    'stamp-selected-16': ('tiff16-two', 'clip:stamp-selected'),
    'merge-group-16': ('tiff16-two', 'clip:merge-group'),
    # Phase 3: layered CMYK files.
    'export-psd-layers-16': ('tiff16-structure', 'save:psd-layers'),
    'open-psd-layers-16': ('none', 'open:psdl16'),
    'open-tiff-alpha-16': ('none', 'open:tiffa16'),
    # The channels batch (#1): the selection kinds, word 85 and one ink plate.
    'plate-load-16': ('tiff16', 'edit:plate-load'),
    'luminance-16': ('tiff16', 'edit:luminance'),
    'similar-16': ('tiff16-selected', 'edit:similar'),
    'native-save-85': ('tiff16-channels', 'save:rz'),
    'native-reopen-85': ('none', 'open:rz85'),
    'move-plate-16': ('tiff16', 'edit:move-plate'),
    'transform-plate-16': ('tiff16', 'edit:transform-plate'),
    # The retouch batch (#1).
    'retouch-clone-16': ('tiff16', 'edit:retouch-clone'),
    'retouch-heal-16': ('tiff16', 'edit:retouch-heal'),
    'spot-heal-16': ('tiff16', 'edit:spot-heal'),
    'dodge-16': ('tiff16', 'edit:dodge'),
    'smudge-16': ('tiff16', 'edit:smudge'),
    'history-brush-16': ('tiff16-history', 'edit:history-brush'),
    'equalize-16': ('tiff16', 'edit:equalize'),
    'equalize-rgb16': ('png16', 'rgb:equalize'),
    'equalize-rgb32': ('png16-float', 'rgb:equalize'),
    # Sample All Layers on Blur and Smudge, 16-bit RGB and CMYK.
    'blur-sal-rgb16': ('png16', 'stroke:blur-sal'),
    'smudge-sal-rgb16': ('png16', 'stroke:smudge-sal'),
    'blur-sal-16': ('tiff16', 'stroke:blur-sal'),
    'smudge-sal-16': ('tiff16', 'stroke:smudge-sal'),
    # The Art History Brush on 8-bit CMYK, the one typed document it runs on.
    'art-history-8': ('tiff8-history', 'stroke:art-history'),
    # Round 2 (#1): the kept-CMYK export's stages.
    'export-cmyk-keep-resize-16': ('tiff16', 'colour:export-keep-resize'),
    'export-cmyk-keep-sharpen-16': ('tiff16', 'colour:export-keep-sharpen'),
    'export-cmyk-keep-depth-8': ('tiff16', 'colour:export-keep-depth-8'),
    'export-cmyk-keep-depth-16': ('tiff8', 'colour:export-keep-depth-16'),
    'export-cmyk-jpeg-keep-staged': ('tiff16', 'colour:export-jpeg-keep-staged'),
    # The artwork batch (#1): type, shape and fill layers in ink.
    'artwork-fill-16': ('tiff16', 'artwork:fill'),
    'artwork-shape-16': ('tiff16', 'artwork:shape'),
    'artwork-edit-16': ('tiff16-fill', 'artwork:edit'),
    # The styles batch (#1): effects and knockout on ink.
    'render-styles-16': ('tiff16-styled', 'render'),
    'merge-styles-16': ('tiff16-styled', 'edit:merge-visible'),
    'render-styled-group-16': ('tiff16-styled-group', 'render'),
    'merge-knockout-16': ('tiff16-knockout', 'edit:merge-down'),
    # Stacked effect instances, CMYK and 16-bit RGB.
    'render-stacked-16': ('tiff16-stacked', 'render'),
    'render-stacked-rgb16': ('png16-stacked', 'render'),
    # Its unseeded twin: Merge Down of a plain layer (no knockout, no seed).
    'merge-down-16': ('tiff16-two', 'edit:merge-down-plain'),
    # The remaining-gaps batch (#1): plates, Apply Image, Gamut Warning, the divisor.
    'plate-fill-16': ('tiff16', 'plate:fill'),
    'plate-gaussian-16': ('tiff16', 'plate:gaussian'),
    'apply-image-cmyk-16': ('tiff16', 'plate:apply-image'),
    'proof-gamut-16': ('tiff16', 'colour:proof-gamut'),
    'render-divisor-16': ('tiff16', 'render-divisor'),
}

# The remaining-gaps batch: each plate call, its plan the reply's `working_memory_plan_bytes`.
PLATE_CALLS = {
    'plate:fill': ('fill', dict(layer=0, target='cyan', fill_mode='selection', color='#000000',
                                x=10, y=10)),
    'plate:gaussian': ('apply_filter', dict(layer=0, target='cyan', filter='blur', sigma=8)),
    'plate:apply-image': ('apply_image', dict(layer=0, target='layer', source_plane='cmyk',
                                              blend_mode='Multiply')),
}

# Round 2 (#1): each kept-CMYK export case's settings and file suffix.
KEPT_SHARPEN = {'radius': 2.0, 'amount': 1.0, 'threshold': 0.0}
KEPT_EXPORTS = {
    'colour:export-keep-resize': ('.tif', {'format': 'tiff', 'bit_depth': 16, 'filter': 'lanczos3',
                                           'size': {'mode': 'scale', 'value': 1.5}}),
    'colour:export-keep-sharpen': ('.tif', {'format': 'tiff', 'bit_depth': 16,
                                            'sharpen': KEPT_SHARPEN}),
    'colour:export-keep-depth-8': ('.tif', {'format': 'tiff', 'bit_depth': 8}),
    'colour:export-keep-depth-16': ('.tif', {'format': 'tiff', 'bit_depth': 16}),
    'colour:export-jpeg-keep-staged': ('.jpg', {
        'format': 'jpeg', 'size': {'mode': 'scale', 'value': 1.5}, 'sharpen': KEPT_SHARPEN,
        'metadata': {'stamp': {'copyright': '(c) 2026 Admission'}}}),
}

# The artwork batch's measured calls; each reply echoes the host's plan.
ARTWORK_CALLS = {
    'artwork:fill': ('add_fill_layer', dict(kind='solid', ink=[20, 40, 60, 10])),
    'artwork:shape': ('add_shape_layer', dict(kind='rect', x=0, y=0, w=SIZE[0], h=SIZE[1],
                                              fill_ink=[20, 40, 60, 10],
                                              stroke_ink=[0, 0, 0, 100], stroke_width=8)),
    'artwork:edit': ('edit_fill_layer', dict(layer=1, ink=[60, 0, 20, 40])),
}

# The styles batch's style (module doc), its colours in ink.
STYLE = {'effects': [
    {'type': 'drop_shadow', 'ink': [0.75, 0.68, 0.67, 0.9], 'blend': 'multiply', 'distance': 40,
     'size': 60},
    {'type': 'outer_glow', 'ink': [1, 0, 0, 0], 'size': 40},
    {'type': 'stroke', 'ink': [0, 0, 0, 1], 'size': 12, 'position': 'outside'},
    {'type': 'color_overlay', 'ink': [0, 0.4, 0, 0], 'opacity': 0.5}]}


def stacked_style(cmyk):
    """The stacked style (module doc): three instances of each repeating kind."""
    colours = (([0.75, 0.68, 0.67, 0.9], '#3a302c'), ([1, 0, 0, 0], '#00a0e9'),
               ([0, 0.4, 0, 0], '#ffa3d2'))
    effects = []
    for i, (ink, hexed) in enumerate(colours):
        slot = {'ink': ink} if cmyk else {'color': hexed}
        effects += [
            dict(type='drop_shadow', distance=20 + 10 * i, size=30 + 10 * i, **slot),
            dict(type='inner_shadow', distance=10 + 5 * i, size=20 + 5 * i, **slot),
            dict(type='stroke', size=6 + 4 * i, position='outside', **slot),
            dict(type='color_overlay', opacity=0.3, **slot),
            dict(type='gradient_overlay', opacity=0.3)]
    return {'effects': effects}


def styled_setup(app, doc, setup):
    """The styles batch's documents (module doc)."""
    def step(tool, **arguments):
        ok, reply = call(app, tool, document_id=doc, **arguments)
        assert ok, (tool, reply)
        return reply

    step('duplicate_layer', index=0)
    if setup in ('tiff16-stacked', 'png16-stacked'):
        step('set_layer_style', layer=1, style=stacked_style(setup == 'tiff16-stacked'))
    elif setup == 'tiff16-styled':
        step('set_layer_style', layer=1, style=STYLE)
    elif setup == 'tiff16-styled-group':
        group = step('group_layers', layers=[0, 1], name='Group')['group']
        # A new group is Pass Through; a style needs an isolated (Normal) one.
        step('set_layer_properties', index=group, blend_mode='Normal')
        step('set_layer_style', layer=group, style=STYLE)
    else:
        step('duplicate_layer', index=1)
        step('set_layer_style', layer=2, style={'knockout': 'deep', 'fill_opacity': 0})

# Phase 4c: the measured call and the plan key, read off `plans_of` (the
# target's unless the key's case says the source's).
CLIP_PLANS = {
    'clip:copy': 'cmyk_copy_scratch_bytes',
    'clip:copy-merged': 'cmyk_copy_merged_scratch_bytes',
    'clip:cut': 'cmyk_cut_scratch_bytes',
    'clip:paste': 'cmyk_paste_scratch_bytes',
    'clip:paste-from-rgb': 'cmyk_paste_convert_scratch_bytes',
    'clip:paste-profile': 'cmyk_paste_convert_scratch_bytes',
    'clip:paste-to-rgb': 'cmyk_paste_to_rgb_scratch_bytes',
    'clip:layer-via-copy': 'cmyk_layer_via_copy_scratch_bytes',
    'clip:layer-via-cut': 'cmyk_layer_via_cut_scratch_bytes',
    'clip:paste-layers-profile': 'cmyk_paste_layers_convert_scratch_bytes',
    'clip:stamp-selected': 'cmyk_stamp_selected_scratch_bytes',
    'clip:merge-group': 'cmyk_merge_group_scratch_bytes',
}

SECOND_PRESS = 'Coated FOGRA39 (ISO 12647-2:2004)'


def second_press(app):
    """The installed press `cmyk-convert-16` and `cmyk-assign-16` move into."""
    ok, listed = call(app, 'list_installed_profiles', kinds=['cmyk'])
    assert ok, listed
    presses = [p for p in listed['profiles'] if p['proofable']]
    chosen = next((p for p in presses if p['name'] == SECOND_PRESS), None) \
        or next((p for p in presses if p['name'] != 'Generic CMYK Profile'), None)
    assert chosen, f'no second proofable CMYK profile is installed: {listed}'
    return chosen['name']


def colour_call(app, document, measured, out, press):
    """Phase 5's measured call: (ok, reply) — every reply carries its plan but `new`."""
    if measured in KEPT_EXPORTS:
        suffix, settings = KEPT_EXPORTS[measured]
        return call(app, 'export_image', document_id=document, path=str(out.with_suffix(suffix)),
                    settings=dict(settings, profile='document'))
    if measured == 'colour:convert':
        return call(app, 'convert_profile', document_id=document, profile='installed', name=press)
    if measured == 'colour:assign':
        return call(app, 'assign_profile', document_id=document, profile='installed', name=press)
    if measured == 'colour:depth':
        return call(app, 'convert_precision', document_id=document, mode='cmyk8')
    if measured == 'colour:new':
        return call(app, 'new_document', width=SIZE[0], height=SIZE[1], unit='px', color_mode='cmyk',
                    depth=16, background='white')
    if measured == 'colour:proof':
        return call(app, 'render_proof', document_id=document, profile='working_cmyk', max_side=64)
    if measured == 'colour:proof-gamut':
        return call(app, 'render_proof', document_id=document, profile='installed', name=press,
                    gamut_warning=True, max_side=64)
    if measured == 'colour:histogram':
        return call(app, 'histogram', document_id=document)
    if measured == 'colour:clip':
        return call(app, 'render', document_id=document, clipping='shadows', max_side=64)
    if measured == 'colour:export-png':
        return call(app, 'export_image', document_id=document, path=str(out.with_suffix('.png')),
                    settings={'format': 'png', 'profile': 'srgb'})
    return call(app, 'export_image', document_id=document, path=str(out.with_suffix('.tif')),
                settings={'format': 'tiff', 'bit_depth': 16, 'profile': 'document'})

CURVES = {'cmyk': [[0, 0], [128, 160], [255, 255]]}
W, H = SIZE
WARP_NODES = [[0, 0]] * 5 + [[60, 40]] + [[0, 0]] * 10
KEYSTONE_CROP = [[0.15 * W, 0.10 * H], [0.85 * W, 0.10 * H], [0.90 * W, 0.90 * H],
                 [0.10 * W, 0.90 * H]]
HUESAT = {'hue': 40}


# The phase-2 calls and the `get_precision` plan each is held to.
EDIT_CALLS = {
    'edit:fill': ('fill_selection', dict(ink=[20, 40, 60, 10], layer=0)),
    'edit:gradient': ('gradient', dict(layer=0, x0=0, y0=0, x1=SIZE[0], y1=SIZE[1],
                                       start_ink=[0, 0, 0, 0], end_ink=[0, 100, 0, 20])),
    'edit:bucket': ('fill', dict(layer=0, x=10, y=10, ink=[20, 40, 60, 10], tolerance=0)),
    'edit:adjust-apply': ('apply_filter', dict(layer=0, filter='curves', params=CURVES)),
    'edit:adjust-add': ('add_adjustment_layer', dict(op='curves', params=CURVES)),
    'edit:new-layer': ('new_layer', dict(name='Ink')),
    'edit:huesat-apply': ('apply_filter', dict(layer=0, filter='hue_saturation', params=HUESAT)),
    'edit:huesat-add': ('add_adjustment_layer', dict(op='hue_saturation', params=HUESAT)),
    'edit:wand': ('select_magic_wand', dict(x=10, y=10, tolerance=32)),
    'edit:range': ('select_color_range', dict(range='sampled', samples16=[[30000, 30000, 30000]],
                                              tolerance=20)),
    'edit:quick': ('quick_select', dict(seeds=[[3000, 2000]], brush_size=201, smooth=1)),
    'edit:refine': ('refine_selection', dict(edge_radius=8, smart_radius=True, output='selection')),
    'edit:stroke-edge': ('stroke_selection', dict(layer=0, width=10, location='outside',
                                                  ink=[0, 0, 0, 100])),
    'edit:apply-mask': ('remove_layer_mask', dict(layer=0, apply=True)),
    'edit:cutout': ('refine_selection', dict(feather=8, output='new_layer_with_mask')),
    'edit:cutout-decontam': ('refine_selection', dict(feather=8, output='new_layer_with_mask',
                                                      decontaminate=True)),
    'edit:bucket-all': ('fill', dict(layer=0, x=10, y=10, ink=[20, 40, 60, 10], tolerance=0,
                                     sample_all_layers=True)),
    'edit:merge-visible': ('merge_visible', {}),
    'edit:stamp-visible': ('stamp_visible', dict(name='Stamp')),
    'edit:merge-down': ('merge_down', dict(layer=2)),
    'edit:merge-down-plain': ('merge_down', dict(layer=1)),
    # Phase 4b: the requests `cmyk_geometry_plans` plans.
    'edit:resize': ('image_size', dict(width=(3 * W + 1) // 2, height=(3 * H + 1) // 2,
                                       filter='catmull-rom')),
    'edit:affine': ('transform_layer', dict(layer=0, rotate=17, sampler='bilinear')),
    'edit:perspective-crop': ('perspective_crop', dict(corners=KEYSTONE_CROP, width=W, height=H,
                                                       sampler='bilinear')),
    'edit:warp': ('warp_layer', dict(layer=0, operation='set', target='pixels', columns=4,
                                     rows=4, displacements=WARP_NODES, filter='bicubic')),
    'edit:liquify': ('liquify', dict(layer=0, strokes=[{
        'tool': 'forward_warp', 'size': 500, 'points': [[W / 2, H / 2], [W / 2 + 300, H / 2]]}])),
    'edit:transform-selected': ('transform_selected_pixels', dict(layer=0, rotate=7,
                                                                  sampler='bilinear')),
    'edit:filter-gaussian': ('apply_filter', dict(layer=0, filter='blur', sigma=8)),
    'edit:filter-surface': ('apply_filter', dict(layer=0, filter='surface_blur', radius=10,
                                                 threshold=15)),
    'edit:filter-dust': ('apply_filter', dict(layer=0, filter='dust_and_scratches', radius=4,
                                              threshold=0)),
    'edit:shadows-apply': ('apply_filter', dict(layer=0, filter='shadows_highlights', params={})),
    'edit:fade': ('fade', dict(opacity=0.5)),
    # The channels batch.
    'edit:plate-load': ('load_selection', {'from': 'plane', 'plane': 'cyan'}),
    'edit:luminance': ('load_selection', {'from': 'plane', 'plane': 'luma'}),
    'edit:similar': ('select_similar', dict(tolerance=32, contiguous=False)),
    'edit:move-plate': ('move_selected_pixels', dict(layer=0, target='cyan', dx=37, dy=23,
                                                     background='#CCCCCC')),
    'edit:transform-plate': ('transform_selected_pixels', dict(layer=0, target='cyan', rotate=7,
                                                               sampler='bilinear',
                                                               background='#CCCCCC')),
    # The retouch batch: the requests `cmyk_retouch_plans` plans.
    'edit:retouch-clone': ('clone_stamp', dict(layer=0, points=[[1500, 2000], [4500, 2000]],
                                               size=100, source_x=1500, source_y=1600,
                                               sample='current')),
    'edit:retouch-heal': ('heal_stroke', dict(layer=0, points=[[3000, 2000]], size=100,
                                              source_x=3000, source_y=1600, sample='current')),
    'edit:spot-heal': ('spot_heal_stroke', dict(layer=0, points=[[3000, 2000]], size=100)),
    'edit:dodge': ('dodge_burn', dict(layer=0, points=[[1500, 2000], [4500, 2000]], size=100,
                                      exposure=50, range='midtones')),
    'edit:smudge': ('smudge_stroke', dict(layer=0, points=[[1500, 2000], [4500, 2000]], size=100,
                                          strength=50)),
    'edit:history-brush': ('history_brush_stroke', dict(layer=0, points=[[1500, 2000], [4500, 2000]],
                                                        size=100, opacity=100)),
    'edit:equalize': ('equalize', dict(layer=0)),
}
# Each stroke's plan is the reply's `working_memory_plan_bytes` (module doc).
STROKE_CALLS = {
    'stroke:blur-sal': ('blur_stroke', dict(layer=0, points=[[100, 100], [5900, 3900]], size=200,
                                            strength=50, sample_all_layers=True)),
    'stroke:smudge-sal': ('smudge_stroke', dict(layer=0, points=[[100, 100], [5900, 3900]],
                                                size=20, strength=50, sample_all_layers=True)),
    'stroke:art-history': ('art_history_brush_stroke', dict(
        layer=0, points=[[100, 100], [5900, 3900]], size=100, style='loose_long', area=500,
        seed=658)),
}
EDIT_PLANS = {
    'edit:fill': 'cmyk_paint_scratch_bytes',
    'edit:gradient': 'cmyk_gradient_scratch_bytes',
    'edit:bucket': 'cmyk_bucket_scratch_bytes',
    'edit:adjust-apply': 'cmyk_adjust_apply_scratch_bytes',
    'edit:adjust-add': 'cmyk_adjust_add_scratch_bytes',
    'edit:new-layer': 'cmyk_add_layer_scratch_bytes',
    'edit:huesat-apply': 'cmyk_huesat_apply_scratch_bytes',
    'edit:huesat-add': 'cmyk_huesat_add_scratch_bytes',
    'edit:wand': 'cmyk_wand_scratch_bytes',
    'edit:range': 'cmyk_range_scratch_bytes',
    'edit:quick': 'cmyk_quick_scratch_bytes',
    'edit:refine': 'cmyk_refine_scratch_bytes',
    'edit:stroke-edge': 'cmyk_stroke_edge_scratch_bytes',
    'edit:apply-mask': 'cmyk_apply_mask_scratch_bytes',
    'edit:cutout': 'cmyk_cutout_scratch_bytes',
    'edit:cutout-decontam': 'cmyk_cutout_decontaminate_scratch_bytes',
    # Gated at the larger of the two stages (module doc).
    'edit:bucket-all': ('cmyk_wand_scratch_bytes', 'cmyk_paint_scratch_bytes'),
    'edit:merge-visible': 'cmyk_merge_scratch_bytes',
    'edit:stamp-visible': 'cmyk_stamp_scratch_bytes',
    'edit:merge-down': 'cmyk_merge_down_scratch_bytes',
    'edit:merge-down-plain': 'cmyk_merge_down_scratch_bytes',
    'edit:resize': 'cmyk_resize_scratch_bytes',
    'edit:affine': 'cmyk_affine_scratch_bytes',
    'edit:perspective-crop': 'cmyk_perspective_crop_scratch_bytes',
    'edit:warp': 'cmyk_warp_scratch_bytes',
    'edit:liquify': 'cmyk_liquify_scratch_bytes',
    'edit:transform-selected': 'cmyk_transform_selected_scratch_bytes',
    'edit:filter-gaussian': 'cmyk_filter_gaussian_scratch_bytes',
    'edit:filter-surface': 'cmyk_filter_surface_scratch_bytes',
    'edit:filter-dust': 'cmyk_filter_dust_scratch_bytes',
    'edit:shadows-apply': 'cmyk_shadows_apply_scratch_bytes',
    'edit:fade': 'cmyk_fade_scratch_bytes',
    'edit:plate-load': 'cmyk_plate_load_scratch_bytes',
    'edit:luminance': 'cmyk_luminance_scratch_bytes',
    'edit:similar': 'cmyk_similar_scratch_bytes',
    'edit:move-plate': 'cmyk_move_plate_scratch_bytes',
    'edit:transform-plate': 'cmyk_transform_plate_scratch_bytes',
    'edit:retouch-clone': 'cmyk_clone_scratch_bytes',
    'edit:retouch-heal': 'cmyk_heal_scratch_bytes',
    'edit:spot-heal': 'cmyk_spot_heal_scratch_bytes',
    'edit:dodge': 'cmyk_dodge_scratch_bytes',
    'edit:smudge': 'cmyk_smudge_scratch_bytes',
    'edit:history-brush': 'cmyk_history_brush_scratch_bytes',
    'edit:equalize': 'cmyk_equalize_scratch_bytes',
}


def rgb_equalize_plan(float_layer):
    """`equalize_apply::equalize_plan` on an RGB canvas-sized layer (module doc)."""
    n, table = W * H, 65536 * 8
    if float_layer:
        return 6 * MIB + 40 * n + 2 * n + math.ceil(2 * n / 4) + table
    return 6 * MIB + 8 * n + 4 * 65536 * 8 + 8 * n + 2 * n + table


# The 16-bit display preparation's charge (`RasterDocument.projectionScratchEstimate()`) for
# the one-layer 6000x4000 16-bit document; restated from gaussian-blur-admission/measure.py's
# PROJECTION_16 (the master copy, with its derivation).
_RENDER_16 = 24 * W * H + (1 << 20)
STRUCTURAL_16 = (_RENDER_16 + math.ceil(_RENDER_16 / 2048)
                 + ((W + 255) // 256 + (H + 255) // 256 - 1) * 32 * 1024)
PROJECTION_16 = STRUCTURAL_16 + (STRUCTURAL_16 * 6 + 15) // 16


def retained(app):
    """`unique_retained_bytes` now (the publication rule's dR)."""
    ok, usage = call(app, 'resource_usage')
    assert ok, usage
    return usage['unique_retained_bytes']

# Phase 4b's mesh warps: the host's admitted plan by arithmetic (module doc).
N = W * H
MARGIN = 4
SOLVER = 8 * (3000 * 3001 // 2) + 4 * MIB
MAP = 20 * 1_000_000 + 2 * MIB
HELD = [(300, 300), (5700, 300), (300, 3700), (5700, 3700)]
PUPPET_PINS = ([{'from': list(p), 'to': list(p)} for p in HELD]
               + [{'from': [3000, 2000], 'to': [3060, 2040]}])
KEYSTONE_QUAD = [{'from': [300, 300, 5700, 300, 5700, 3700, 300, 3700],
                  'to': [500, 300, 5500, 300, 5800, 3700, 200, 3700]}]


def mesh_plan(width, height, expansion=2):
    grow = abs(expansion) + 1
    w, h = width + 2 * grow, height + 2 * grow
    plane = w * h
    return plane + plane * 8 + (w // 4 + 2) * (h // 4 + 2) * 24 + max(w, h) * 24 + MIB


MESH_COMMON = {'mesh:puppet': mesh_plan(W, H) + SOLVER + MAP + 2 * MIB,
               'mesh:perspective': MIB + SOLVER + MAP + 2 * 256 + 2 * MIB}
MESH_CALLS = {'mesh:puppet': ('puppet_warp', dict(layer=0, pins=PUPPET_PINS)),
              'mesh:perspective': ('perspective_warp', dict(layer=0, quads=KEYSTONE_QUAD))}


def mesh_warp_plan(measured, dest):
    """The host's admitted plan over the warped layer's `dest` (module doc)."""
    w, h = dest
    d = w * h
    tiles = math.ceil(w / 256) * math.ceil(h / 256)
    return MESH_COMMON[measured] + 10 * N + d * (10 + MARGIN) + 10 * d + 64 * tiles + 24 * N


def owned(output, name, executable):
    """An owned `HostApp` (private preference folders, a dummy API key,
    `DR_BACKGROUND`, its own scratch MCP socket), stopped by its own context manager."""
    app = support.HostApp(output / f'app-{name}', output / f'recovery-{name}', executable)
    return app


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def cmyk_tiff(path, width, height, bits, icc, flat=False, alpha=False):
    """A little-endian chunky CMYK TIFF (Photometric 5), one strip, a
    horizontal ink gradient with per-column noise — or, `flat`, one ink;
    `alpha` adds a fifth, unassociated sample (ExtraSamples 2), a ramp."""
    top = 255 if bits == 8 else 65535
    seed = 0x9E3779B9
    columns = []
    for x in range(width):
        seed = (seed * 1103515245 + 12345) & 0xFFFFFFFF
        noise = (seed >> 16) % 17
        columns.append(top // 3 if flat else ((x * top) // width + noise) % (top + 1))
    row = bytearray()
    for x in range(width):
        c = columns[x]
        values = (c, (top - c), (c // 2 + top // 4), (c // 3))
        if alpha:
            values += ((x * top) // max(width - 1, 1),)
        row += bytes(values) if bits == 8 else struct.pack(f'<{len(values)}H', *values)
    data_length = len(row) * height
    header = 8
    samples = 5 if alpha else 4
    bits_at = header + data_length
    icc_at = bits_at + 2 * samples
    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, samples, struct.pack('<I', bits_at)), (259, 3, 1, struct.pack('<HH', 1, 0)),
        (262, 3, 1, struct.pack('<HH', 5, 0)), (273, 4, 1, struct.pack('<I', header)),
        (277, 3, 1, struct.pack('<HH', samples, 0)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', data_length)), (284, 3, 1, struct.pack('<HH', 1, 0)),
    ] + ([(338, 3, 1, struct.pack('<HH', 2, 0))] if alpha else []) + [
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    ifd_at = icc_at + len(icc) + (len(icc) & 1)
    with Path(path).open('wb') as out:
        out.write(b'II*\x00' + struct.pack('<I', ifd_at))
        for _ in range(height):
            out.write(row)
        out.write(struct.pack(f'<{samples}H', *([bits] * samples)))
        out.write(icc)
        if len(icc) & 1:
            out.write(b'\x00')
        out.write(struct.pack('<H', len(entries)))
        for tag, kind, count, value in entries:
            out.write(struct.pack('<HHI', tag, kind, count) + value)
        out.write(struct.pack('<I', 0))


def fixtures(output, executable):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    icc = GENERIC.read_bytes()
    made = {'tiff16': folder / 'cmyk16.tif', 'tiff8': folder / 'cmyk8.tif',
            'png16': folder / 'rgb16.png', 'jpeg8': folder / 'cmyk.jpg',
            'psd16': folder / 'cmyk16.psd', 'rz16': folder / 'cmyk16.rz',
            'flat16': folder / 'flat16.tif', 'rz83': folder / 'cmyk16-masked.rz',
            'rz84': folder / 'cmyk16-structure.rz', 'tiff16b': folder / 'cmyk16-b.tif',
            'swop16': folder / 'swop16.tif', 'psdl16': folder / 'cmyk16-layers.psd',
            'tiffa16': folder / 'cmyka16.tif', 'rz85': folder / 'cmyk16-channels.rz'}
    if not made['tiff16'].is_file():
        cmyk_tiff(made['tiff16'], *SIZE, 16, icc)
    if not made['tiff8'].is_file():
        cmyk_tiff(made['tiff8'], *SIZE, 8, icc)
    if not made['flat16'].is_file():
        cmyk_tiff(made['flat16'], *SIZE, 16, icc, flat=True)
    # Phase 4c: a second Generic file (a paste target of its own) and, when
    # installed, a SWOP one.
    if not made['tiff16b'].is_file():
        cmyk_tiff(made['tiff16b'], *SIZE, 16, icc)
    if not made['swop16'].is_file() and SWOP.is_file():
        cmyk_tiff(made['swop16'], *SIZE, 16, SWOP.read_bytes())
    if not made['tiffa16'].is_file():
        cmyk_tiff(made['tiffa16'], *SIZE, 16, icc, alpha=True)
    if not made['png16'].is_file():
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        subprocess.run([str(binary), 'png16', str(SIZE[0]), str(SIZE[1]), str(made['png16'])],
                       check=True, capture_output=True, timeout=600)
    if not all(made[k].is_file() for k in ('jpeg8', 'psd16', 'rz16', 'rz83', 'rz84', 'psdl16',
                                            'rz85')):
        with owned(output, 'author-app', executable) as app:
            ok, opened = call(app, 'open_document', path=str(made['tiff16']))
            assert ok, opened
            doc = opened['document']['id']
            for result in (call(app, 'save_copy', document_id=doc, path=str(made['jpeg8']), format='jpeg'),
                           call(app, 'export_psd', document_id=doc, path=str(made['psd16']), mode='flattened'),
                           call(app, 'save_copy', document_id=doc, path=str(made['rz16']), format='rz')):
                assert result[0], result
            masked(app, doc)
            ok, saved = call(app, 'save_copy', document_id=doc, path=str(made['rz83']), format='rz')
            assert ok, saved
            ok, opened = call(app, 'open_document', path=str(made['rz16']))
            assert ok, opened
            plain = opened['document']['id']
            structured(app, plain)
            ok, saved = call(app, 'save_copy', document_id=plain, path=str(made['rz84']), format='rz')
            assert ok, saved
            unfeathered(app, plain)
            ok, saved = call(app, 'export_psd', document_id=plain, path=str(made['psdl16']),
                             format='psd', mode='layers')
            assert ok, saved
            # An open of an already open path answers that document, so the word-85 document
            # starts from a copy of the 16-bit TIFF's own `.rz`.
            fresh = folder / 'cmyk16-for-85.rz'
            fresh.write_bytes(made['rz16'].read_bytes())
            ok, opened = call(app, 'open_document', path=str(fresh))
            assert ok, opened
            channelled = opened['document']['id']
            channels(app, channelled)
            ok, saved = call(app, 'save_copy', document_id=channelled, path=str(made['rz85']),
                             format='rz')
            assert ok, saved
    return made


def channels(app, doc):
    """Two alpha channels (the left and the right half) and a rectangular vector mask on
    the layer: the word-85 document."""
    def step(tool, **arguments):
        ok, reply = call(app, tool, document_id=doc, **arguments)
        assert ok, (tool, reply)
        return reply

    for name, x in (('Left', 0), ('Right', SIZE[0] // 2)):
        step('select_rect', x=x, y=0, width=SIZE[0] // 2, height=SIZE[1])
        step('save_selection', name=name)
    step('deselect')
    step('set_vector_mask', layer=0, subpaths=[{'closed': True, 'anchors': [
        [300, 200], [SIZE[0] - 300, 200], [SIZE[0] - 300, SIZE[1] - 200], [300, SIZE[1] - 200]]}])


def masked(app, doc):
    """A Curves layer with a feathered reveal-all mask: the word-83 document."""
    for name, arguments in (('add_adjustment_layer', dict(op='curves', params=CURVES)),
                            ('add_layer_mask', dict(kind='reveal_all')),
                            ('set_mask_properties', dict(feather=20))):
        ok, reply = call(app, name, document_id=doc, **arguments)
        assert ok, (name, reply)


def structured(app, doc):
    """Two layers in a Pass Through group with a feathered mask, the top one
    Multiply, and a filled layer clipped above the group: the word-84
    document."""
    def step(tool, **arguments):
        ok, reply = call(app, tool, document_id=doc, **arguments)
        assert ok, (tool, reply)
        return reply

    step('duplicate_layer', index=0)
    group = step('group_layers', layers=[0, 1], name='Group')['group']
    step('add_layer_mask', layer=group, kind='reveal_all')
    step('set_mask_properties', layer=group, feather=20)
    step('set_layer_properties', index=1, blend_mode='Multiply')
    step('set_active_layer', index=group)
    step('new_layer', name='Clip')
    clip = len(step('get_document')['layers']) - 1
    step('select_all')
    step('fill_selection', ink=[0, 0, 0, 30], layer=clip)
    step('deselect')
    step('set_layer_clipped', layer=clip, clipped=True)


def unfeathered(app, doc):
    """Phase 3: the word-84 document's group mask with its feather cleared,
    because a PSD mask stores no feather and Export as PSD refuses one."""
    ok, state = call(app, 'get_document', document_id=doc)
    assert ok, state
    for layer in state['layers']:
        if layer.get('has_mask'):
            ok, reply = call(app, 'set_mask_properties', document_id=doc, layer=layer['index'],
                             feather=0)
            assert ok, reply


def settle(pid, timeout=180):
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def plans_of(app, document):
    ok, precision = call(app, 'get_precision', document_id=document)
    if ok and precision.get('mode') == 'rgba16':
        # `render-stacked-rgb16`: a 16-bit RGB document's `plans` hold
        # its projection charge alone.
        return precision['plans']
    assert ok and precision.get('mode') == 'cmyk', precision
    return precision['plans']


def open_plan(plans, path):
    return SIZE[0] * SIZE[1] * plans['cmyk_open_bytes_per_pixel'] + path.stat().st_size + MIB


def measure_clip_case(output, executable, made, label, case, baseline):
    """A phase-4c case (module doc): the setup copies on the case's own
    private pasteboard, the plan is read before the measured call."""
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.cmyk-admission.{os.getpid()}.{label}'
    try:
        with owned(output, label, executable) as app:
            def opened(key, **extra):
                ok, reply = call(app, 'open_document', path=str(made[key]), **extra)
                assert ok, reply
                return reply['document']['id']

            def step(document, tool, **arguments):
                ok, reply = call(app, tool, document_id=document, **arguments)
                assert ok, (tool, reply)
                return reply

            plan_document = None
            if measured.startswith('clip:paste'):
                source_key = {'clip:paste-from-rgb': 'png16', 'clip:paste-profile': 'swop16',
                              'clip:paste-layers-profile': 'swop16'}.get(measured, 'tiff16')
                if source_key == 'swop16' and not made['swop16'].is_file():
                    raise AssertionError(f'the SWOP profile is missing: {SWOP}')
                source = opened(source_key, **({'precision': 'preserve'}
                                               if source_key == 'png16' else {}))
                if measured == 'clip:paste-layers-profile':
                    step(source, 'copy', mode='layers', layers=[0])
                else:
                    step(source, 'select_all')
                    step(source, 'copy', layer=0)
                target = (opened('png16', precision='preserve') if measured == 'clip:paste-to-rgb'
                          else opened('tiff16b'))
                document = target
                plan_document = source if measured == 'clip:paste-to-rgb' else target
            else:
                document = opened('tiff16')
                plan_document = document
                if setup == 'tiff16-two':
                    step(document, 'duplicate_layer', index=0)
                if measured == 'clip:merge-group':
                    step(document, 'group_layers', layers=[0, 1], name='Group')
                if measured in ('clip:copy', 'clip:copy-merged', 'clip:cut',
                                'clip:layer-via-copy', 'clip:layer-via-cut'):
                    step(document, 'select_all')
            plan = None
            if not baseline:
                plan = plans_of(app, plan_document)[CLIP_PLANS[measured]]
            pid = app.process.pid
            settle(pid)
            pre, _ = footprint(pid, reset=True)
            if baseline:
                ok, reply = call(app, 'resource_usage')
            else:
                name, arguments = {
                    'clip:copy': ('copy', dict(layer=0)),
                    'clip:copy-merged': ('copy_merged', {}),
                    'clip:cut': ('cut', dict(layer=0)),
                    'clip:layer-via-copy': ('layer_via_copy', dict(layer=0)),
                    'clip:layer-via-cut': ('layer_via_cut', dict(layer=0)),
                    'clip:stamp-selected': ('stamp_selected', dict(layers=[0, 1])),
                    'clip:merge-group': ('merge_group', dict(layer=2)),
                }.get(measured, ('paste', dict(placement='in_place')))
                ok, reply = call(app, name, document_id=document, **arguments)
            settle(pid)
            post, peak = footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if not ok:
                    record['refusal'] = str(reply)[:800]
                record['plan'] = plan
    finally:
        os.environ.pop('DR_PASTEBOARD_NAME', None)
    return record


def measure_case(output, executable, made, label, case, baseline):
    setup, measured = CASES[case]
    if measured.startswith('clip:'):
        return measure_clip_case(output, executable, made, label, case, baseline)
    record = dict(case=case, label=label, baseline=baseline)
    with owned(output, label, executable) as app:
        document = None
        history_state = None
        if setup in ('tiff16', 'tiff16-two', 'png16', 'png8', 'tiff8', 'flat16', 'tiff16-masked',
                     'tiff16-huesat', 'tiff16-selected', 'tiff16-layer-masked',
                     'tiff16-structure', 'tiff16-blurred', 'tiff16-channels', 'tiff16-history',
                     'png16-float', 'tiff16-fill', 'tiff16-styled', 'tiff16-styled-group',
                     'tiff16-knockout', 'tiff16-stacked', 'png16-stacked', 'tiff8-history'):
            key = {'png16': 'png16', 'png8': 'png16', 'tiff8': 'tiff8', 'png16-float': 'png16',
                   'flat16': 'flat16', 'png16-stacked': 'png16',
                   'tiff8-history': 'tiff8'}.get(setup, 'tiff16')
            arguments = dict(path=str(made[key]))
            if setup in ('png16', 'png8', 'png16-float', 'png16-stacked'):
                arguments['precision'] = 'rgba8' if setup == 'png8' else 'preserve'
            ok, opened = call(app, 'open_document', **arguments)
            assert ok, opened
            document = opened['document']['id']
            if setup == 'tiff16-two':
                ok, dup = call(app, 'duplicate_layer', document_id=document, index=0)
                assert ok, dup
            if setup == 'tiff16-masked':
                masked(app, document)
            if setup == 'tiff16-structure':
                structured(app, document)
                if measured == 'save:psd-layers':
                    unfeathered(app, document)
            if setup in ('tiff16-styled', 'tiff16-styled-group', 'tiff16-knockout',
                         'tiff16-stacked', 'png16-stacked'):
                styled_setup(app, document, setup)
            if setup == 'png16-float':
                ok, converted = call(app, 'convert_precision', document_id=document, mode='rgba32',
                                     structure='preserve')
                assert ok, converted
            if setup == 'tiff16-history':
                ok, history = call(app, 'list_history', document_id=document)
                assert ok, history
                history_state = history['current_state_id']
            if setup in ('tiff16-blurred', 'tiff16-history', 'tiff8-history'):
                ok, blurred = call(app, 'apply_filter', document_id=document, layer=0,
                                   filter='blur', sigma=8)
                assert ok, blurred
            if setup == 'tiff16-channels':
                channels(app, document)
            if setup == 'tiff16-fill':
                ok, added = call(app, 'add_fill_layer', document_id=document, kind='solid',
                                 ink=[20, 40, 60, 10])
                assert ok, added
            if setup == 'tiff16-huesat':
                ok, added = call(app, 'add_adjustment_layer', document_id=document,
                                 op='hue_saturation', params=HUESAT)
                assert ok, added
            if measured in ('edit:fill', 'edit:transform-selected', 'edit:move-plate',
                            'edit:transform-plate', 'plate:fill'):
                ok, selected = call(app, 'select_all', document_id=document)
                assert ok, selected
            if setup in ('tiff16-selected', 'tiff16-layer-masked'):
                ok, selected = call(app, 'select_rect', document_id=document, x=0, y=0,
                                    width=SIZE[0] // 2, height=SIZE[1])
                assert ok, selected
            if setup == 'tiff16-layer-masked':
                for name, extra in (('add_layer_mask', dict(layer=0, kind='from_selection')),
                                    ('deselect', {})):
                    ok, reply = call(app, name, document_id=document, **extra)
                    assert ok, (name, reply)
        open_arguments = {}
        # Phase 2's edit plans (and phase 3's) are read before the measured call.
        plan = None
        if measured == 'open:psd16':
            # The compatibility report is the setup; the interval holds the open alone.
            ok, report = call(app, 'inspect_psd', path=str(made['psd16']))
            assert ok and report.get('import_precision') == 'cmyk', report
            open_arguments = dict(psd_mode='composite', psd_fingerprint=report['source_fingerprint'])
        if measured == 'open:psdl16':
            # Phase 3: the report is the setup and its layered plan the gate.
            ok, report = call(app, 'inspect_psd', path=str(made['psdl16']))
            assert ok and report.get('import_precision') == 'cmyk' \
                and report.get('layered_available'), report
            open_arguments = dict(psd_mode='layers', psd_fingerprint=report['source_fingerprint'])
            plan = None if baseline else report['layered_plan_bytes']
        press = (second_press(app) if measured in ('colour:convert', 'colour:assign',
                                                   'colour:proof-gamut') else None)
        if measured == 'save:psd-layers' and not baseline:
            ok, report = call(app, 'inspect_psd_export', document_id=document, format='psd',
                              mode='layers')
            assert ok and report.get('depth') == 'cmyk', report
            plan = report['working_memory_plan_bytes']
        if measured.startswith('edit:') and not baseline:
            keys = EDIT_PLANS[measured]
            plans = plans_of(app, document)
            plan = max(plans[k] for k in keys) if isinstance(keys, tuple) else plans[keys]
        pid = app.process.pid
        # The 16-bit RGB Equalize row's publication rule needs dR (module doc).
        publication = measured == 'rgb:equalize' and setup == 'png16' and not baseline
        before = retained(app) if publication else None
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        out = output / f'{label}-out'
        if baseline:
            ok, reply = call(app, 'resource_usage')
        elif measured.startswith('open:'):
            path = made[measured.split(':')[1]]
            ok, reply = call(app, 'open_document', path=str(path), **open_arguments)
        elif measured == 'to-cmyk':
            ok, reply = call(app, 'convert_mode', document_id=document, mode='cmyk',
                             profile='file', path=str(GENERIC))
            plan = reply.get('working_memory_plan_bytes') if ok else None
        elif measured == 'to-rgb':
            ok, reply = call(app, 'convert_mode', document_id=document, mode='rgb')
            plan = reply.get('working_memory_plan_bytes') if ok else None
        elif measured in ('render', 'render-divisor'):
            # `render` answers an image, not JSON: read its error flag only.
            result = app.client.rpc('tools/call', {'name': 'render', 'arguments': dict(
                document_id=document, preview_divisor=2 if measured == 'render-divisor' else 1,
                max_side=64)})
            ok = not result.get('isError')
            reply = next((c.get('text', '') for c in result.get('content', [])
                          if c.get('type') == 'text'), '') if not ok else {}
        elif measured == 'flatten':
            ok, reply = call(app, 'flatten_image', document_id=document)
        elif measured.startswith('edit:'):
            name, arguments = EDIT_CALLS[measured]
            if history_state:
                arguments = dict(arguments, state_id=history_state)
            ok, reply = call(app, name, document_id=document, **arguments)
        elif measured.startswith('stroke:'):
            name, arguments = STROKE_CALLS[measured]
            ok, reply = call(app, name, document_id=document, **arguments)
            if ok:
                plan = reply.get('working_memory_plan_bytes')
        elif measured.startswith('plate:'):
            name, arguments = PLATE_CALLS[measured]
            ok, reply = call(app, name, document_id=document, **arguments)
            if ok:
                plan = reply.get('working_memory_plan_bytes')
        elif measured == 'rgb:equalize':
            ok, reply = call(app, 'equalize', document_id=document, layer=0)
            plan = rgb_equalize_plan(setup == 'png16-float')
            if publication:
                plan = max(plan, PROJECTION_16)
        elif measured.startswith('colour:'):
            ok, reply = colour_call(app, document, measured, out, press)
            if ok and measured != 'colour:new':
                plan = reply.get('working_memory_plan_bytes')
        elif measured.startswith('mesh:'):
            name, arguments = MESH_CALLS[measured]
            ok, reply = call(app, name, document_id=document, **arguments)
        elif measured.startswith('artwork:'):
            name, arguments = ARTWORK_CALLS[measured]
            ok, reply = call(app, name, document_id=document, **arguments)
            plan = reply.get('working_memory_plan_bytes') if ok else None
        elif measured == 'save:psd-layers':
            ok, reply = call(app, 'export_psd', document_id=document, path=str(out.with_suffix('.psd')),
                             format='psd', mode='layers')
        elif measured == 'save:psd':
            ok, reply = call(app, 'export_psd', document_id=document, path=str(out.with_suffix('.psd')),
                             mode='flattened')
        else:
            fmt = measured.split(':')[1]
            ok, reply = call(app, 'save_copy', document_id=document,
                             path=str(out.with_suffix('.' + {'jpeg': 'jpg'}.get(fmt, fmt))), format=fmt)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if publication:
            record['retained_growth'] = retained(app) - before
        if not baseline:
            if not ok:
                record['refusal'] = reply[:800]
            else:
                if measured == 'colour:new':
                    plan = plans_of(app, reply['document']['id'])['cmyk_new_scratch_bytes']
                if measured.startswith('mesh:'):
                    ok_state, state = call(app, 'get_document', document_id=document)
                    row = state['layers'][0] if ok_state else {}
                    record['dest'] = [row.get('width'), row.get('height')]
                    plan = mesh_warp_plan(measured, (row['width'], row['height']))
                elif measured.startswith('open:'):
                    plans = plans_of(app, reply['document']['id'])
                    if measured in ('open:rz16', 'open:rz83', 'open:rz84', 'open:rz85'):
                        plan = plans['decode_scratch_bytes'] + plans['reopen_retained_bytes']
                    elif measured == 'open:tiffa16':
                        plan = open_plan(plans, made['tiffa16']) + 2 * SIZE[0] * SIZE[1]
                    elif measured == 'open:psdl16':
                        pass                    # `inspect_psd`'s layered plan, read before.
                    else:
                        plan = open_plan(plans, made[measured.split(':')[1]])
                elif plan is None:
                    plans = plans_of(app, document)
                    # The render plans first: a 16-bit RGB row's `plans` hold only them.
                    plan = plans['projection_scratch_bytes'] if measured in (
                        'render', 'render-divisor') else {
                            'flatten': plans['cmyk_flatten_scratch_bytes'],
                            'save:tiff': plans['cmyk_save_scratch_bytes'],
                            'save:psd': plans['cmyk_psd_save_scratch_bytes'],
                            'save:jpeg': max(plans['cmyk_save_scratch_bytes'],
                                             12 * SIZE[0] * SIZE[1] + GENERIC.stat().st_size + MIB),
                            'save:rz': plans['native_scratch_bytes']}[measured]
                record['plan'] = plan
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ.get('DR_VERIFY_APP',
                                                    'build/Build/Products/Release/Doctored.app')))
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    arguments = parser.parse_args()
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-cmyk-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(LIMIT)
    app = arguments.app if arguments.app.is_absolute() else ROOT / arguments.app
    executable = (app / 'Contents/MacOS/Doctored' if app.suffix == '.app' else app).resolve()
    made = fixtures(output, executable)
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=LIMIT,
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size)
                            for name, path in made.items()},
                  records=[])
    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                try:
                    record = measure_case(output, executable, made, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)
    table, passed, counted = {}, True, 0
    for case in selected:
        measured = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(measured) and all(r.get('ok') for r in measured)
        plans = {r.get('plan') for r in measured}
        plan = next(iter(plans)) if len(plans) == 1 else None
        growth = [r['retained_growth'] for r in measured if 'retained_growth' in r]
        if plan is not None and growth:
            plan += min(growth)
        max_e = max((r.get('E', 0) for r in measured), default=0)
        baseline_e = max((r.get('E') or 0 for r in baselines), default=0)
        need = math.ceil(1.25 * max_e)
        skipped = ok and plan is not None and max_e <= baseline_e
        row = dict(E=[r.get('E') for r in measured], baseline_E=[r.get('E') for r in baselines],
                   plan=plan, need=need, ratio=(plan / max_e) if plan and max_e else None,
                   skipped=skipped, passed=ok and plan is not None and plan >= need)
        if not skipped:
            counted += 1
            passed &= row['passed']
        table[case] = row
    if selected and not counted:
        passed = False
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        verdict = 'SKIP (no rise)' if row['skipped'] else 'PASS' if row['passed'] else 'FAIL'
        print(f'{case:18} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-cmyk: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
