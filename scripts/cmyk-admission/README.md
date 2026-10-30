# #547 CMYK admission drive

Every memory plan the CMYK document mode added (phases 1 to 5) is held to the
measured-admission rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E
is `ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the
footprint has been still for 2 s, so the host's display preparation of the
result is inside the interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/cmyk-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s
`HostApp`, `DR_BACKGROUND=1`, a dummy API key, private preference folders) at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline
twin that calls `resource_usage` instead, `--runs` times. The fixtures are 24 MP
(6000 × 4000): 16- and 8-bit chunky CMYK TIFFs (and a 16-bit CMYKA one) carrying
macOS's Generic CMYK Profile, written by the drive with no uniform tile; a CMYK JPEG,
a flattened and a layered CMYK PSD and `.rz` files the app writes from the 16-bit
TIFF; and a 16-bit RGB PNG
from `scripts/host-still-admission/author.swift`.

The cases and the plan each is held to are listed in `measure.py`'s docstring.
The figures behind the plans are the per-pixel terms beside the code they
charge: `cmyk_io::OPEN_BYTES_PER_PIXEL`, `SAVE_BYTES_PER_PIXEL` and
`SAVE_PSD_BYTES_PER_PIXEL`; `cmyk_convert::TO_CMYK_BYTES_PER_PIXEL`,
`TO_RGB_BYTES_PER_PIXEL` and `FLATTEN_BYTES_PER_PIXEL`;
`cmyk_native::REOPEN_MARGIN_PER_PIXEL`; phase 2's (#547 edit) are
`cmyk_paint::CANVAS_BYTES_PER_PIXEL` (`fill-16`, `gradient-16`, `bucket-16`),
`cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL` (`adjust-apply-16`, `adjust-add-16`
and, as the larger of it and the plane's 10 B/px, `new-layer-16`; raised from
16 to 20 after the first measurement missed all three); `render-masked-16` adds a feathered
mask's preparation to the view's charge, and `native-save-83` /
`native-reopen-83` hold a word-83 document (a Curves layer with a mask).
Hue/Saturation on ink (#547) holds K and reads no profile, so it has no memo:
`huesat-apply-16`, `huesat-add-16` and `render-huesat-16` (a `{"hue":40}`
layer under `render`) are charged as Levels and Curves are (the apply's and
add's description and display terms, the stack's masks and row buffers); the
per-ink memo option (b) charged is gone. Phase 4a's
(#547 structure) are `cmyk_merge`'s plan, `FLATTEN_BYTES_PER_PIXEL` over the
larger of the frame and the canvas (`merge-16` for Merge Visible,
`stamp-16` for Stamp Visible, read as `cmyk_merge_scratch_bytes` and
`cmyk_stamp_scratch_bytes`), the stack's group and clip row buffers in
`render-structure-16` (a Pass Through group with a feathered mask, a Multiply
layer inside it and a clipped layer above it), and `native-save-84` /
`native-reopen-84`, which hold that word-84 document. The phase-2 and phase-4a
plans are read from `get_precision` before the measured call. The view (`render-16`) is charged the
host's `projectionScratchEstimate` (`ProjectionMargin`), which `get_precision`
reports as `projection_scratch_bytes`. The selection batch (#547) adds
`wand-16`, `range-16`, `quick-16` and `refine-16` (the `cmyk_select` plans:
the RGB plan's 40 B a pixel and held input plus the ink or view canvas),
`stroke-edge-16` (`cmyk_stroke`, charging `cmyk_paint::CANVAS_BYTES_PER_PIXEL`
over the canvas), `apply-mask-16` (`precision_mask_apply`'s CMYK arm),
`cutout-16` and `cutout-decontam-16` (`cmyk_cutout`, the latter with
`precision_foreground::FOREGROUND_BYTES_PER_PIXEL`; the first measurement
missed `apply-mask-16` and `cutout-16`, so both now charge
`cmyk_paint::CANVAS_BYTES_PER_PIXEL` over the canvas too) and `bucket-all-16`, gated
at the larger of the wand's and the paint's plans. Phase 5 (#547 colour) adds
`cmyk_profile::CONVERT_BYTES_PER_PIXEL` (`cmyk-convert-16`, beside the ink memo
and the view), `cmyk_profile::DEPTH_BYTES_PER_PIXEL` (`cmyk-depth-8`),
`cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL` again (`cmyk-assign-16`,
`new-cmyk-16`: the display preparation of the reassigned or new document),
`cmyk_proof::PROOF_BYTES_PER_PIXEL` (`proof-cmyk-16`),
`cmyk_tone_analysis::CLIP_BYTES_PER_PIXEL` (`clip-display-16`; the tone
analysis, `tone-analysis-16`, charges its fixed planes and the stack only) and
`cmyk_export::TO_RGB_EXPORT_BYTES_PER_PIXEL` / `KEEP_BYTES_PER_PIXEL`
(`export-cmyk-png`, `export-cmyk-tiff-keep`); their plans are a reply's
`working_memory_plan_bytes`, or `get_precision`'s `cmyk_new_scratch_bytes`. Phase 4b (#547 geometry) adds
`resize-16`, `affine-16`, `perspective-crop-16`, `warp-16`, `liquify-16` and
`transform-selected-16`, gated on the descriptor's `cmyk_geometry_plans` (the typed geometry
plan over five planes with the CMYK source and adoption copies; `cmyk_warp`'s warp and
Liquify plans; `cmyk_selected_pixels`' twice-the-u16-arm plan with its
`CMYK_SELECTED_SAMPLE_BYTES`), and `puppet-16` and `perspective-warp-16`, gated on the
host's admitted plan restated by arithmetic in `measure.py`'s docstring
(`precision_puppet::mesh_warp_plan`'s CMYK arm + the 24n projection). The first 4b
measurement missed `resize-16` (1.002×) and `liquify-16` (1.059×): a CMYK Image Size now
takes the geometry plan's `TRANSFORM_HEADROOM`, and the CMYK Liquify plan charges
`cmyk_paint::CANVAS_BYTES_PER_PIXEL` over the canvas for the host's display preparation.
The adjustments and filters (#547) add `filter-gaussian-16`, `filter-surface-16`
and `filter-dust-16` (`cmyk_filter`'s plan: the RGB pass's own scratch, 34 B a
layer pixel for the two `L` passes and the ink, the remapped coverage and
`DISPLAY_BYTES_PER_PIXEL` over the canvas), `shadows-apply-16` (a destructive
Shadows/Highlights: the CMYK apply plan plus the guide plane's term) and
`fade-16` (`cmyk_fade`'s 20 B a layer pixel after a blur), each read from
`get_precision` before the measured call. The retouch batch (#547) adds
`retouch-clone-16`, `retouch-heal-16` (the two-pass plan: the RGB pass's own
plan twice, 34 B a destination pixel, the plate and its passes and
`DISPLAY_BYTES_PER_PIXEL` over the canvas), `spot-heal-16` (the RGB synthesis
plan at four channels plus the plate), `dodge-16`, `smudge-16` and
`history-brush-16` (`cmyk_retouch_brush`'s two-pass plans) and `equalize-16`
(the CMYK adjustment apply's terms plus the table), each read from
`get_precision` before the measured call, and `equalize-rgb16` /
`equalize-rgb32`, gated on `equalize_apply`'s RGB plans restated by
arithmetic in `measure.py`'s docstring; the 16-bit row, whose E holds the
16-bit display preparation, by gaussian-blur-admission's P16 publication rule
(max(plan, `PROJECTION_16`) + dR). A miss raises the missing plan's
term; the 1.25 bound is never widened.

Phase 4c (#547 copy and paste) adds
`copy-16`, `copy-merged-16`, `cut-16`, `paste-16`, `paste-from-rgb-16`,
`paste-profile-16`, `paste-to-rgb-16`, `layer-via-copy-16`, `layer-via-cut-16`,
`paste-layers-profile-16`, `stamp-selected-16` and `merge-group-16`, each held
to its `get_precision` key (`cmyk_clipboard::cmyk_clipboard_plans`): the reads
charge `precision_clipboard_host`'s host stage over the CMYK payload, the pastes
`cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL` over the canvas beside the
structure term, a conversion `cmyk_convert::TO_CMYK_BYTES_PER_PIXEL` /
`TO_RGB_BYTES_PER_PIXEL` and the memo, Layer Via `COPIED_PLANE_COPIES` and the
ink erase's plan, the layers paste `LAYER_PASTE_MARGIN_PER_PIXEL`, and Stamp
Selected / Merge Group `cmyk_merge`'s plan. Every 4c case runs on its own
private pasteboard (`DR_PASTEBOARD_NAME`), and the two profile cases read a
second CMYK profile, Adobe's U.S. Web Coated (SWOP) v2. A miss raises the
missing plan's term; the 1.25 bound is never widened.

Phase 3 (#547 layered files) adds `export-psd-layers-16` (`export_psd` layers of the unfeathered
`render-structure-16` document, gated on `inspect_psd_export`'s
`working_memory_plan_bytes`: `psd_export_cmyk::LAYERED_SAVE_BYTES_PER_PIXEL` over the
canvas beside the record walk), `open-psd-layers-16` (that file opened with psd_mode
layers in a fresh app, gated on `inspect_psd`'s `layered_plan_bytes`:
`psd_import_cmyk::LAYERED_OPEN_BYTES_PER_PIXEL` over the canvas beside the decode peak)
and `open-tiff-alpha-16` (a CMYKA16 TIFF the drive writes; `cmyk_io::open_scratch` with
the alpha's decoded bytes). The first phase-3 measurement missed `export-psd-layers-16` (0.964×), so
`LAYERED_SAVE_BYTES_PER_PIXEL` went from 42 to 68 (1.261×).

The channels batch (#547) adds `plate-load-16`, `luminance-16` and `similar-16` (the `cmyk_select`
plans), `native-save-85` / `native-reopen-85` (two alpha channels and a vector mask, word 85; the
first reopen measurement missed at 1.233×, so `cmyk_native::REOPEN_MARGIN_PER_PIXEL` went from 24
to 25) and `move-plate-16` / `transform-plate-16` (`cmyk_selected_plate`'s plan,
`cmyk_move_plate_scratch_bytes` / `cmyk_transform_plate_scratch_bytes`); the backdrop wand has no
MCP or menu path, so `wand-16` stands for its plan.
A miss raises the missing plan's term; the 1.25 bound is never widened.

Round 2 (#547 kept-CMYK export) adds `export-cmyk-keep-resize-16`, `export-cmyk-keep-sharpen-16`,
`export-cmyk-keep-depth-8`, `export-cmyk-keep-depth-16` (the 8-bit fixture) and
`export-cmyk-jpeg-keep-staged`, each gated on `export_image`'s `working_memory_plan_bytes`:
`cmyk_export_ink`'s staged plan (the on-paper ink, `resize_cmyk`'s copies over five planes, the
two `L` sharpen passes and `OutputSharpen::plan_bytes`, and the output at
`SAVE_BYTES_PER_PIXEL` + `KEEP_BYTES_PER_PIXEL`). A miss raises `KEEP_BYTES_PER_PIXEL`.
The remaining-gaps batch re-derived the staged plan as the LARGEST stage peak plus the result
(the stages run one after another), so the five rows are re-measured against it.

The remaining-gaps batch (#547) adds `plate-fill-16` and `plate-gaussian-16` (one ink plate as a
scalar target, gated on the reply's `working_memory_plan_bytes`: `ScalarEdit::plan` with its plate
terms), `apply-image-cmyk-16` (Apply Image onto the CMYK layer, plate for plate; the reply's
`NativeChannelMath.scratch`), `proof-gamut-16` (Gamut Warning against the second press;
`cmyk_proof`'s plan) and `render-divisor-16` (a preview-divisor-2 render, gated as `render-16` on
`projection_scratch_bytes`). A miss raises `DISPLAY_BYTES_PER_PIXEL` or `PROOF_BYTES_PER_PIXEL`.

The artwork batch (#547) adds `artwork-fill-16` (`add_fill_layer` solid in ink over the canvas),
`artwork-shape-16` (`add_shape_layer` rect over the canvas with a fill and an 8 px stroke in ink)
and `artwork-edit-16` (`edit_fill_layer` recolouring a solid fill), each gated on the plan the host
admits for the render and echoes as the reply's `working_memory_plan_bytes`
(`NativeArtworkEvaluation.scratchBytes` + `cmykMarginPerPixel` × the raster + 64 × the profile); a
miss raises `cmykMarginPerPixel`.

The styles batch (#547 layer effects and knockout on ink) adds `render-styles-16` (a duplicated
layer styled with a Multiply drop shadow, an outer glow, an outside stroke and a colour overlay in
ink; `render` gated on `projection_scratch_bytes`, which holds `cmyk_style::styled_scratch`),
`merge-styles-16` (that document's `merge_visible`, `cmyk_merge_scratch_bytes`),
`render-styled-group-16` (the two layers grouped, made Normal, and the group styled; the group's ink
frame), `merge-knockout-16` (`merge_down` of a deep-knockout layer at Fill 0 over the Background,
gated on `cmyk_merge_down_scratch_bytes`: the members' plan beside the seed's,
`cmyk_merge::cmyk_seed_scratch_bytes`) and its unseeded twin `merge-down-16`. The first
measurement missed `merge-knockout-16` (1.147×: the seeded Merge Down peaked 589 MB where the
unseeded twin peaked 433 MB), so `FLATTEN_BYTES_PER_PIXEL` went from 28 to 31.

The stacked effect instances batch adds `render-stacked-16` (the duplicated layer styled with
`stacked_style`: three instances each of drop shadow, inner shadow, outside stroke, colour overlay
and gradient overlay, in ink) and `render-stacked-rgb16` (the same on the 16-bit RGB PNG in sRGB
colours), each `render` gated on `projection_scratch_bytes`.
The drive prints one JSON record per app, then a row per case (plan, need,
ratio, E of each run, baseline E) and `gate M-cmyk: PASS|FAIL`, and writes
`receipt.json` (app and script hashes, fixtures, every record, the table) in the
output folder. A row whose max E is not above its baseline twins' is skipped;
the gate fails when every selected row was skipped. Exit 0 only on a pass.
