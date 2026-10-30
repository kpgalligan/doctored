# CMYK document mode (#547): shipped runs

The settled record of [#547](https://github.com/kpgalligan/doctored-dev/issues/547)'s CMYK phases: decisions in force and
each shipped phase's few lines, added by the batch that ships it. [cmyk-mode-status.md](cmyk-mode-status.md) keeps the
head, unshipped contracts and `## Open items`. Takes live under `doctored-fixtures/photoshop/`, runners in
`scripts/photoshop-probe/`.

## Decisions in force

- **Native storage, not RGB with a tag.** A CMYK document is a `PrecisionDocument` with its `cmyk` policy set (the `float`
  field's pattern) and `Content::CmykRaster` leaves: straight CMYKA16 ink, 0 = none, 65535 = 100 %, tiled, Arc-shared, 10
  B/px. The policy keeps the profile's BYTES verbatim (a save embeds what was opened), the declared depth (8 stores codes
  ×257 and writes `round(v/257)`) and the view link. `PrecisionDocument::profile` is Display P3: the VIEW's space, not the
  colour.
- **One gate.** `build_cmyk` admits CMYK raster leaves, CMYK adjustment records (`Content::CmykAdjustment`) and Normal /
  Pass Through groups of them at any depth, each with an optional mask, clipping, a `CMYK_BLEND_MODES` mode and a
  layer style (Fill; since the styles batch every effect, knockout and Blend Interior), and a description (since DRDC
  87); an RGB record, Blend If, another mode ("The <name> blend mode"), a placed source or a warp refuses by name (`first_refusal`, postorder).
  `rebuilt` routes every CMYK edit through it; `build` refuses a CMYK record, `scene_for` refuses (no RGB scene),
  `dr_precision_doc_edit` refuses every op outside the CMYK set before parsing (`cmyk_route`). The host asks
  `CmykDocument` FIRST (`DocumentPrecision`). Admitted (`precision_capability::CMYK_FAMILIES` / `.cmykTool`;
  `require_cmyk` admits a target any line lists): the layer property and structure families; paint and erase; fill_stroke;
  layer masks; adjustments, Desaturate, filters, Fade; render, sample, export, convert; source selections; the structure
  families (group, clipped, blend, Fill `layer_style`, merges, Stamp Visible); every geometry family, Transform Mask,
  selected pixels; the clipboard family, Stamp Selected, Duplicate to Document; `profile`, `proof`, `analysis`; clone,
  heal, retouch_brush; `global_light`; geometric selections, guides, history, recovery. Everything else: "<op> is unavailable in a CMYK
  document".
- **One compositor.** `cmyk_stack` is the ONE CMYK row compositor over the record TREE in `i64` canvas rows: a raster
  composites through its mode (`over_mode`; Normal is phase 1's `over` bit for bit) at `alpha × opacity × mask × fill`; an
  adjustment moves the ink below by `k = opacity × mask` through its mode (`tone_mix_mode`, alpha kept, alpha-0 skipped,
  RGB's `adjust_backdrop_sample16` rule); an isolated group composites its children into its own row buffer, then at
  `alpha × opacity × fill × mask`; Pass Through composites over a row copy and gates back by `opacity × mask`; a clip unit
  draws the base at Normal / 1 with its Fill, clamps members' alpha to the base's `alpha × mask`, composites by the base's
  mode and opacity; a Pass Through base with a visible member, or a clipped Pass Through member, isolates. Its scratch
  (masks prepared once, memos, a 24 B/px row buffer per group and clipped record) is charged by every plan that builds
  one.
- **Blend modes on ink.** `ink_blended` evaluates a mode on `L = 1 − ink` through `blend.rs`: separable `B` per plate on
  all four; Hue, Saturation, Color on the CMY `L` triple with the backdrop's K, Luminosity with the source's K;
  Darker/Lighter Color a whole-sample pick by `lum(L_CMY)·L_K`; mixed by the W3C step (`blend::composite_blended`).
  Dissolve by `dissolve_threshold`. Fill multiplies into the source alpha as
  `precision_style_render::composite_pixels_into` does (the special eight not modelled). `CMYK_BLEND_MODES` /
  `NativeLayerBlending.cmykModes` hold all 27 modes. Divide keeps 0 at 0 / 0 and Vivid Light's halves take
  a black source to 0 and a white one to 1 first, at every depth (`blend-edge-547/take1`). Hard Mix's exact tie
  (`|cb + cs − 1| ≤ 2⁻²²`) is 1 iff `cb − cs > 1/32768`, else 0, at every depth (`gray-luminance/take1`, HM-ORD).
- **Merges and Stamp Visible** take pixels from `cmyk_merge::cmyk_frame_image` (the stack over the members' union frame,
  off-canvas kept, or the canvas) as straight ink, with the RGB merges' structural rules verbatim; the floors count CMYK
  rasters. Plan: `FLATTEN_BYTES_PER_PIXEL` over max(frame, canvas) + a frame row + the stack's scratch
  (`cmyk_merge_scratch_bytes`, `cmyk_stamp_scratch_bytes`); a seeded (D7) merge adds the source stack and two rows
  (`cmyk_merge_down_scratch_bytes`, styles batch).
- **Geometry on ink.** Every resampler runs on the five CMYKA16 planes through the RGB kernels generalised over channel
  count (C = 4 is RGB's arithmetic bit for bit), premultiplied as RGB (no Photoshop take): transform, resize, Perspective
  Crop, the mesh warps. Plans charge 10 B/px output plus source and adoption copies. A CMYK warp applies at once and keeps
  NO binding (float's rule: `warp_info` null, Clear/Bake no change). Selected pixels and Liquify run THE RGB walk over two
  ink views, `[C, M, Y, A]` and `[K, 0, 0, A]`, joined (alpha must agree). No format change. The CMYK content box (Reveal
  All, align/distribute, ⌘T) is alpha × enabled mask, memoized; Trim compares composite ink (all five codes).
- **Copy and paste.** Copy, Cut and Copy Merged make an RZCB v4 payload (v3's layout, storage 3 = CMYKA16, a
  declared-depth word; only `cmyk_clipboard_codec` reads it, the RGB decoder refuses v4, RGB payloads unchanged) of the
  raw ink (a group's raw projection; an adjustment record refuses: "use Copy Merged"); Cut erases as `CmykPaintEdit` at
  opacity 1; a whole-layer Copy of a type or shape layer carries its editable section after the depth word (v3's rules).
  Same profile bytes keep the ink bit for bit; another CMYK profile converts under Convert (`CmykToCmyk`), keeps the ink
  under Don't Convert. Across modes pixels always convert, relative + BPC; Don't Convert refuses ("Don't Convert keeps
  numbers within one colour mode; …"). The core never mixes modes in a layers clip (refused by name); the host converts it
  first (`CmykLayerClip`). A CMYK TIFF on the pasteboard becomes a CMYK payload (untagged: the destination's profile, else
  the working CMYK); an RGB image pastes from its own space. A paste that changes mode or profile reports
  `color_conversion` and a " (CMYK → RGB)"-style history suffix. Each operation's plan is a
  `cmyk_<op>_scratch_bytes` descriptor.
- **Stamp Selected and Merge Group** (every depth). Stamp Selected: one group, or two or more records sharing a parent,
  composited into ONE new leaf above the topmost member, originals untouched; no shortcut (⌥⌘E is Quick Export). Merge
  Group: a visible group's RAW projection (`raw_group_document`) as one leaf keeping the group's slot, id, name, opacity,
  mode (Pass Through → Normal), style, Blend If, mask, clipping, locks and link; ⌘E on one group runs it (retitled "Merge
  Group").
- **Conversions carry the tree** (`cmyk_convert_tree`): groups, masks, clipping, modes and Fill both ways. RGB → CMYK
  blocks (first in postorder) an RGB adjustment, an enabled effect, knockout, Blend If, a mode CMYK lacks, a placed image
  or a warp; CMYK → RGB blocks a CMYK adjustment record anywhere. Both plans answer `flattens`.
- **Artwork on ink.** A CMYK document's descriptions store ink (text v5, shape v6, fill v2: `[c, m, y, k, alpha]` 0…1, a
  gradient's `stop_inks` one per STORED stop) beside hex fields holding its sRGB appearance (relative + BPC). The core
  renders it (`cmyk_artwork`): coverage is the RGB path's own, fill and interior ink exact, a colour glyph's sRGB
  separated relative + BPC. Editors and MCP work on the appearance (`cmykDisplay`) and commit through `cmykArtwork`: an
  unchanged colour keeps its ink (layer, swatch or MCP `ink`/`fill_ink`/`stroke_ink`; a gradient stop by stored index).
  Mode, Convert and cross-profile or cross-mode pastes drop descriptions in the core; the host re-renders the captured
  layers in converted colours (unless the conversion flattens). Geometry, paint, merges and Convert clear a description
  where RGB does; duplicate, Assign and Depth keep it; PSD writes pixels with RGB's loss line. No Photoshop take.
- **Ink, not colour.** Painting writes ink (`cmyk_paint`, `CmykPaintEdit`): `ink` = straight `[c, m, y, k, alpha]`, the
  RGB tools' planes and plan keys, Normal over at `ink_alpha × opacity × coverage`, alpha frozen under Preserve
  Transparency or a lock, through `blend.rs`'s RGB functions. A colour crosses the boundary once, via `dr_cmyk_color` (the
  document's profile, else the working CMYK): the Picker's C/M/Y/K set the swatch's INK exactly
  (`SharedToolState.foregroundInk`/`backgroundInk`, cleared by any RGB-side edit; swap swaps them, D clears them so black
  converts to rich black), the eyedropper keeps the composite's ink, and a colour with no stored ink converts through the
  profile.
- **Levels and Curves on ink.** RGB's description schema; on CMYK only `levels` and `curves`. Levels acts on `L = 1 − ink`
  (a CMYK PSD's stored domain): top-level = composite stage, optional `c`/`m`/`y`/`k` stages, channel then composite,
  `ink' = 1 − out`. Curves points are INK, 0–255 for 0–100 % (keys `cmyk`, `c`, `m`, `y`, `k`):
  `ink' = S_cmyk(S_ink(255·ink))/255`, natural spline. Drawn curves are `cmyk_map`, `c_map`… (256 output ink codes,
  plate's map then composite's; a map key beside a point key is malformed): 8-bit reads the byte maps, 16-bit each plate's
  composed 15-bit table. RGB keys refuse by name. Dialogs list CMYK and the four plates; Curves' axes and readout are ink
  %, and Pencil, Smooth and map Apply work on ink.
- **Hue/Saturation holds K** (owner, 2026-10-14), RGB's params. Not Colorize: the RGB kernel on the CMY
  stored domain, `out = apply([1 − C, 1 − M, 1 − Y])`, `C' = 1 − out_r`…, `K' = K` exactly (K-only greys stay K-only).
  Colorize (CZ1, fitted on the take): the naive `(1 − ink)(1 − K)` RGB through the kernel, `K' = 1 − max(out)`,
  `C' = (1 − out_r − K')/(1 − K')`. No profile, memo or B2A refusal; `CmykAdjustment::apply` is the one evaluator for
  stack and destructive apply. Targeted Adjustment stays off; a mask or channel target keeps its scalar refusals.
- **Adjustments on ink.** Every Adjustments and New Adjustment Layer item offered in CMYK runs on ink,
  the same at 8 and 16 bits; `cmyk_adjust_rgb::parse_cmyk` is the one dispatch (`CmykAdjustment::Ink`); records are word
  83's kind-3 descriptions. P4 = the RGB kernel on `L = 1 − ink`, `[L_c, L_m, L_y]` then `[L_k; 3]`: Invert, Posterize
  (16-bit bins), Brightness/Contrast (Saturation refused), Shadows/Highlights (guide from `ink_tone = luma709(L_CMY)·L_K`,
  `cmyk_adjust_ink`'s ONE CMYK tone, shared by Threshold and Gradient Map). C3K = the kernel on `[L_c, L_m, L_y]`, K held:
  Color Balance, Photo Filter (`ink` for `color`), Color Lookup. Native: Channel Mixer ON THE INK,
  `ink_o = Σ q(w_j)·ink_j + q(constant)` (M-I); Selective Color (memberships of the `L` triple, nudges on ink); Threshold
  (`ink_tone ≥ level` → paper, else K 100 % only); Gradient Map (`ink_tone`, ink stops, `gradient_field::segment`, no
  dither); Replace Color (`inks`, distance over C, M, Y, K, Hue/Saturation's master on the C3K triple). Shadows/Highlights
  and Replace Color are pixels-only. Auto Tone / Color = `per_channel` Auto, Auto Contrast
  `monochromatic`, 0.1 % clipped, as a CMYK Levels.
- **Filters on ink** (`cmyk_filter`). Two RGBA16 passes of the RGB selected-filter path, `[L_c, L_m, L_y, α]` and
  `[L_k; 3, α]` (PLATE-L, per take1); frames, banding, seeds and no-op rule are the RGB path's; Add Noise's K pass seeds
  with XOR `0x9E3779B97F4A7C15`; Clouds take `foreground_ink` / `background_ink`; a transparency lock restores source
  alpha; an output at α 0 keeps its ink. Edit ▸ Fade is `cmyk_fade` (`fade_pixel`'s rule on ink); Repeat Last Filter by
  the step's gate. Refusals (`ADJUSTMENT_REFUSALS`, `FILTER_REFUSALS`, `CmykAdjustFilter`) are the status doc's
  Still-refused list.
- **Selections.** Magic Wand and Paint Bucket compare the INK as naive RGB (PN7): `R = (1 − C)(1 − K)` (G from M, B from
  Y), through 8-bit codes on an 8-bit document, then the `ReferenceRange` (u16 codes, the host's 8-bit t × 257),
  four-connected through the one `flood`; source `composite` or `layer`. The All Layers bucket is that composite region,
  then a CMYK `paint_blend`. Color Range, Quick Selection and the edge stage read the VIEW (`cmyk_view_canvas`, Display P3
  as the Lab profile); the Vision selections are given the view. `alpha`, `color` (a plate: `1 − ink`) and `similar` (PN7)
  read ink, `luminance` the composite's sRGB luma (LP), a backdrop is `cmyk_backdrop`. Select and Mask's New Layer
  (`cmyk_cutout`) is the composite's ink as a new top raster masked by the matte; Decontaminate Colors runs
  `foreground_solve` on C/M/Y then K. Stroke… with nothing selected strokes the alpha edge (`cmyk_stroke`), with a
  selection the band, in ink. Apply Layer Mask bakes `round(alpha·m)`, ink kept. No format change. One ink plate is a scalar target (`cmyk_plate_scalar`): every RGB-plane
  edit, `read_plane` and `scalar_statistics` on its stored `1 − ink` (grey g leaves ink 1 − g); `luminance` reads a layer too.
- **Photoshop take1** (`cmyk-select-547/take1`, 26.10.0; VALID). Wand: Photoshop admits single-plate steps of t + 1 (t =
  10, 32; ±1 at t = 0 dark), every K pair and the 8- and 20-code GCR twins, and refuses C/M/Y pairs at Δ = t − 1 and t
  (mid sheet); PN7 superseded P11; `fixtures/photoshop_cmyk_wand` pins it region by region. Color Range (CIE76 in the
  view's D50 Lab) MISSES the ordering bound, shipped as declared. Decontaminate Colors RUNS in CMYK.
- **One colour engine.** `cmyk_color` holds the two legs (`ToDevice`/`RgbToCmyk`, `CmykToRgb`); the proofer and the flat
  open call them.
- **Defaults:** relative colorimetric with BPC; the destination is the working CMYK
  (`WorkingCMYK.swift`: U.S. Web Coated (SWOP) v2 when installed, else macOS's Generic CMYK Profile), read from the
  system, never bundled (no profile licence). Mode ▸ RGB goes to the RGB working space. Edit ▸ Color Settings… (⇧⌘K, keys
  `ColorWorkingCMYK`, `ColorRGBPolicy`, `ColorCMYKPolicy`, `ColorIntent`, `ColorBPC`) is the one home: working RGB and
  CMYK (Automatic, an installed profile or a loaded file), open policies (RGB Convert = `adoptWorkingSpace`, Preserve
  skips it, Off assigns the working RGB; CMYK Preserve keeps, Off assigns the working CMYK, Convert converts in the open),
  intent and BPC; every reader of them reads it; `WorkingCMYK` reinstalls on `didChange`. Gray and dither omitted; no MCP
  setter (the view-preference exemption). `ColorSettingsStore` keeps the keys in `UserDefaults`, or under
  `DR_COLOR_SETTINGS_DIR` (every owned instance) the launch arguments then a private `color-settings.plist`.
- **Profile edits (`cmyk_profile`).** Assign = a new `CmykPolicy`, samples untouched (identical bytes = `None`); Convert =
  each raster leaf at any depth (`map_cmyk_tree`) through `CmykToCmyk` (source A2B → abs scale → BPC → destination B2A,
  memoized per ink), alpha, masks, records, ids and metadata carried, `flatten` first on request, depth kept; Convert to
  an RGB profile is `convert_mode`'s path. 16→8 rounds every raster and mask sample to `round(v/257)·257`; 8→16 changes
  only the declared depth. No format change.
- **The CMYK proof (`cmyk_proof`).** Per composite ink: device = ink under Preserve CMYK Numbers, else
  `CmykToCmyk(doc → proof)`; then `soft_proof::ProofTail` into Display P3 (info `space:"display-p3"`, tagged
  `ColorProfile.displayP3`); plate p is the grey `255 − round(255·device[p])`. A non-CMYK proof profile refuses on CMYK; on RGB, `plate` with a non-CMYK proof profile and `preserve_numbers` refuse. Gamut Warning on CMYK flags an ink when the proof
  converts and `gamut_delta(proof, A2B(ink)) > 5`, overlaid opaque, `out_of_gamut` its count; preserved numbers flag nothing.
  Working CMYK on a CMYK document preserves numbers. `ProofSetup.reconciled(isCmykDocument:)` runs before every canvas request.
  `soft_proof::gamut_delta` is the ONE gamut rule (proof, `separate`, Info's "!", the Picker's alarm, Gamut Warning).
- **Tone tools (`cmyk_tone_analysis`).** Histograms of `L = 1 − ink` per plate (weight coverage × alpha; composite = the
  four summed); Auto = clipped endpoints per C, M, Y with K held at (0, 1) (`per_channel`, the default), or one
  pair from the summed histogram on all four (`monochromatic`: Auto Contrast moves K); the host writes a composite
  stage only when all four pairs are equal; eyedroppers: white = paper, black =
  `srgb_to_ink(black)`, grey = the sample's Lab with a* = b* = 0 through B2A1 (relative, no BPC); ⌥ clipping: highlights
  where every listed plate is 0 ink, shadows where any is 100 %. `.acv`/`.alv` ink channels decode via `cmyk_preset_file`.
- **Exports (`cmyk_export`, `cmyk_export_ink`).** Profile `document` with JPEG or TIFF keeps CMYK: the composite on paper,
  resampled by `resize_cmyk`, output-sharpened PLATE-L on 16-bit codes, then the depth step (8-bit `(v+128)/257`, never
  dithered), encoded under the request's metadata policy at the output size; a default request is the save's bytes. A TIFF
  is 8 or 16 bits; refused by name: another TIFF depth, and dither on an 8-bit TIFF of a 16-bit document (a JPEG ignores
  dither). The JPEG is the host's transcode of the 8-bit intermediate (`cmykJPEG`), its packets MEASURED from the written
  file. Anything else flattens on paper through `CmykToRgb` into u16 RGB, then the RGB stages. Save a Copy offers `.rz`,
  JPEG and TIFF.
- **The view.** The CMYK → Display P3 leg (relative + BPC) sampled in LINEAR light on a 33⁴ grid, quadrilinear, then
  clamped and encoded through a 4097-sample table. Bound: ≤ 1/255 of the exact leg at every grid cell centre; on a miss
  the grid is raised, never the bound. 14.2 MB per policy, charged by every view plan.
- **Opens.** A CMYK TIFF/JPEG opens as CMYK when `dr_path_cmyk_source` answers it (`cmyk_gate`, plus an embedded profile
  or a set working profile); the host decides BEFORE the 16-bit prompt (`.ask`/`.preserve` → CMYK, `.rgba8` → sRGB). An
  untagged file with the working slot set is ASSIGNED it under "The image is CMYK with no embedded colour profile, so
  the working CMYK profile “…” was assigned as it was read."; with the slot empty it opens as the naive numbers labelled
  sRGB. A CMYK TIFF may be chunky or planar, 8/16-bit, its first extra sample read as associated or unassociated alpha
  (`cmyk_tiff_read`, the ONE decode); further extras are counted and named, not read. A colour-mode-4 PSD, PSB or
  big-endian layered TIFF reports `import_precision:"cmyk"` through `psd_import::report`'s ONE record walk
  (`CMYK_GENERAL_LIMITATIONS`) and opens its saved composite (mode 1; merged alpha not read, said so) or its layers
  (mode 0, `open_cmyk_layers`). Both, and Import Layers, apply the CMYK policy (`openPrecisionPSD`).
- **Saves.** TIFF (CMYK8/16, no ExtraSamples) and the JPEG intermediate (8-bit CMYK TIFF ImageIO transcodes) write the
  composite flattened onto paper at the declared depth; Export as PSD writes PSD, PSB or layered TIFF, Layers or
  Flattened; `.rz` keeps every layer.
- **Files.** Layered import keeps raster ink + transparency (`max − stored`, 8-bit ×257; no −1 channel = opaque), masks,
  Normal/Pass Through/moded groups, clipping, the 27 modes, opacity, Fill, locks, labels, CMYK Levels/Curves (point form;
  `levl`/`curv` records 0–4 = composite, C, M, Y, K, points reflected) and Hue/Saturation (`hue2`); any other adjustment
  is omitted with `CMYK_KIND`, type/shape/smart objects import as pixels,
  a missing ink channel refuses. Channel id 3 is admitted only for colour 4. Export writes the same back (ink as
  `max − ink`, adjustments never baked) with a five-plane composite (`on_paper` ink + alpha); Flattened is the zero-record
  colour-4 composite (TIFF: the plain CMYK TIFF). A CMYK document needs no projection. Plans:
  `LAYERED_OPEN_BYTES_PER_PIXEL` 27 and `LAYERED_SAVE_BYTES_PER_PIXEL` 68 over the canvas; the alpha TIFF's `open_scratch`
  adds the extras' decoded bytes.
- **Retouch on ink.** The shipped RGB16 walk on two canvas-sized `L` pass documents (`cmyk_passes`: `[L_c, L_m, L_y, α]`,
  `[L_k; 3, α]`; a locked record's passes carry the lock), joined by `join_sample` (α must agree): Dodge/Burn (DB-P4, all
  four plates; Protect Tones PT-V, `curve(V)/V` per group, K the plain curve, every depth), Blur, Sharpen, Smudge, the
  History Brush (the state split too), Heal. Sponge: pass A only, K exact. Clone: both passes cloned onto a blank pass,
  then `over_mode` at the opacity. Spot Heal, Content-Aware Fill, Remove, Move: synthesis at C = 4 on ink (the kernels
  generic over C; C = 3 bit-identical). Plate: Current = the layer's ink on paper; Current & Below / All =
  `cmyk_frame_image` of a copy with `retouch_cut16`'s records at opacity 0. Paint, Fill, Gradient and Stroke… take every
  CMYK mode; Red Eye, Auto-Align and Auto-Blend stay refused (`CmykRetouch`).
- **Equalize** (every depth): EQ-SUM-MIN, one table from the summed plane histograms (CMYK: the four `L` plates), weight
  coverage × α, `round(max·(cdf − cdf(v_min))/(N − cdf(v_min)))`, 256 or 65536 levels; float on the display encoding,
  out-of-range samples kept. `selected` (default, lerped) or `entire` (unmeasured). 8-bit RGB via
  `OperationCapture` (any target), typed in the core.
- **Styles on ink (styles batch).** Effects composite in the ONE stack through the kernel `cmyk_style_row` (D3–D6 of
  `style_knockout` on ink, `over_mode` on `L = 1 − ink`); planes are the shared u8 renderer's (`cmyk_style`). Every colour
  slot may carry `ink` [c, m, y, k] (bevel `highlight_ink`/`shadow_ink`, a stop's `ink`), authoritative on CMYK: its hex is
  rewritten to the ink's sRGB at the edit, Assign and PSD import (`canonical_hex`); an ink-less slot converts its hex at
  composite time and at PSD export (`with_all_inks`); an ink-less black drop shadow, inner shadow, satin or bevel shadow
  stores K100 (take1 FITTED D-K). An 8-bit document holds an ink on the grid `round(p·2.55)`. RGB setters and
  CMYK → RGB drop inks; Convert to Profile and a converting layers paste map inks through the raster leg. Merge Down /
  Layers of a knockout or grouped unit are seeded from the source stack's capture (D7). CMYK PSDs carry `lfx2` with `CMYC`,
  `knko`/`infx` and twelve adjustment kinds (`grdm` in `psd_adjustment_grdm`); a kind or style Photoshop cannot hold refuses
  Layers mode in the report and the save alike.

- **Blend If on ink (Blend If batch).** Gray, Cyan, Magenta, Yellow, Black on rasters and isolated groups
  (`style_blend_if::source_weight_ink`): both sides first rounded to the stored code at the depth; a plate reads
  `code_max − code`, Gray `round(code_max·lumW3C)` of the naive RGB; knees INC `[lo0 − 1, lo1, hi0, hi1 + 1]` (HALF
  `±½` on Yellow, the fit's tie-break), ×257 at 16 bits (extrapolated). Red/Green/Blue refuse by name; Gray converts
  both ways, a plate blocks RGB Color. CMYK PSDs carry the five slots both ways.

- **Format.** DRDC 82: the first word inside 46, storage tag 3, raw LE CMYKA16 rasters (`w·h·10`), table `CMYK` (schema 1,
  depth, profile) before `LIDS`; every prior version reads, a pre-82 reader refuses. A ladder, each rung ⊇ the one below,
  checked both ways over every record (under it "… requires DRDC n", a rung holding none "invalid native CMYK … word"):
  83 a mask or CMYK adjustment record (kind 3); 84 (`needs_structure`) a group, clipped record, non-Normal mode or style;
  85 (`needs_channels`) alpha channels or a vector mask; 86 (`needs_effects`) an enabled effect, knockout, Blend Interior
  or Blend If (`LBIF` codes 0, 4–7); 87 (`needs_descriptions`) an `ADSC` description; 88 a stacked style. CMYK records never
  carry words 62, 64 or 71; the RGB words 91 (`LREF`) and 90 (a pattern overlay) follow the rung.

- **Pattern Overlay on ink.** Each texel converts through the Colour Overlay's leg (`leg.ink`, then `on_grid`),
  premultiplied by its alpha, into an ink PYRAMID (`pattern_tile::ink_level`) sampled where the RGB plane put the tile
  (`InkColor::Pattern`); `ink_level_bytes` is its exact scratch term. CMYK PSDs write RGB `Patt`/`Pat2` records.

## Shipped

- **Phase 1 — open, view, convert, save, layer structure** (`ad8db093`): opens, view, saves, Image ▸ Mode, the
  plate rows, and the CMYK routes of the read, sample, render and save tools.
- **Phase 2 — edit** (`5e36e43e`): New Layer, masks, the paint and fill tools in ink (MCP `ink`), Levels and Curves on
  ink, the Picker's and Info's CMYK, `dr_cmyk_color`, DRDC 83.
- **Hue/Saturation on ink** (`94ea2822`): destructive and as a layer; its K rule is `0aab1139`'s.
- **Selections, Stroke… and Apply Layer Mask** (`174cb195`): the wand on ink, the Vision and view-based selections, Color
  Range, Select and Mask, the All Layers bucket, Stroke… in ink, Layer Mask ▸ Apply.
- **Phase 4a — structure** (`3caea697`): groups, clipping, the 27 modes, Fill Opacity, the merges, Stamp Visible,
  conversions carrying the tree, DRDC 84. Take1 (`cmyk-blend-547/take1`): four modes MISS, shipped as built (open item).
- **Phase 5 — colour** (`650776d8`): Color Settings…, Assign / Convert to Profile, 8 / 16 Bits, New in CMYK, proofing and
  plates, exports, tone tools; on RGB the gamut alarms; MCP `separate_color`, `new_document` `color_mode`.
  `DISPLAY_BYTES_PER_PIXEL` 20 → 21. The Auto take MISSED on K, held since `0aab1139`.
- **Phase 4b — geometry** (`b034d253`): sizes, crops, Trim, Reveal All, rotation, Free Transform, the warps, Liquify,
  Move, align/distribute. Image Size's plan gained `TRANSFORM_HEADROOM`, Liquify's `CANVAS_BYTES_PER_PIXEL`.
- **The black plate, Color Settings isolation, drawn curves** (`0aab1139`): Hue/Saturation and per-channel Auto hold
  K, wand and bucket on PN7, `DR_COLOR_SETTINGS_DIR`, Curves' Pencil/Smooth/map Apply on RGB and CMYK. Take
  `cmyk-k-547/take1` VALID; PN7's residual is pinned in `fixtures/photoshop_cmyk_wand/confirm.json`.
- **Adjustments and filters** (`bc28e4cc`): every Adjustments, New Adjustment Layer, Auto and Filter item offered
  in CMYK, Fade, Repeat Last Filter. Takes `cmyk-adjust-filter-547` take1/take2: six items MISS, shipped as built
  (open items).
- **Phase 4c — copy and paste** (2026-10-14): the clipboard and layer transfer family, a CMYK TIFF on the pasteboard,
  the paste's colour-conversion disclosure (`color_conversion` and the history suffix); Stamp Selected and Merge Group at
  8/16/32 bits and on CMYK; no DRDC change.
- **Phase 3 — files** (2026-10-14): layered CMYK PSD, PSB and TIFF open and save; CMYK TIFF alpha, extra samples, planar.
  Take2 opened Doctored's files (`cmyk-psd-547/`). `LAYERED_SAVE_BYTES_PER_PIXEL` 42 → 68 after a measured miss.
- **Channels, Quick Mask, vector masks, Select commands, one plate** (2026-10-15): alpha channels, Quick Mask, paths and
  vector masks, the Select commands, the Magnetic Lasso, luminosity masks, Move / ⌘T on one plate (`cmyk_selected_plate`);
  DRDC 85. Take `cmyk-channels-547/take1`: plate load exact, composite load (LP) fitted, Grow/Similar unmeasurable
  (Photoshop: "not currently available"). `REOPEN_MARGIN_PER_PIXEL` 24 → 25.
- **Painting and retouching on ink, Equalize at every depth** (2026-10-15): the toning, focus, history, clone and healing
  tools, Patch, Remove, Content-Aware Fill/Move, paint and fill modes on ink; Equalize at RGB 8/16/32 and CMYK, MCP
  `equalize`; no DRDC change. Takes `cmyk-retouch-547` take1/take2: VALID; MISS, shipped as built (open items).
- **Round 2 — kept-CMYK export** (2026-10-15): a kept JPEG or TIFF resizes, output-sharpens, changes depth and applies the
  metadata policy; `export_psd`/`inspect_psd_export` name each vector mask written as pixels (RGB and CMYK).
- **Type, shape and fill layers in ink** (2026-10-15): the Type and shape tools, Fill Layers, Artwork Colors…, MCP
  text/shape/fill tools with `ink`, `fill_ink`, `stroke_ink`; DRDC 87; `dr_precision_doc_cmyk_artwork`,
  `dr_cmyk_artwork_colors`.
- **Styles** (2026-10-15): every layer effect in ink with Global Light, knockout and Blend Interior on CMYK rasters and
  isolated groups, through merges, stamps, flatten, conversions, copy/paste and proof; the Layer Style sheet, MCP, Actions;
  CMYK PSD/PSB/TIFF carry effects, knockout and the nine adjustment records; DRDC 86. take1 (`cmyk-styles-547`): effects
  and knockout BUILT, default FITTED D-K, Blend If MISS (next batch).
- **Remaining gaps** (2026-10-15): one ink plate painted, filled, filtered and adjusted; Fill/Stroke Path in ink; Apply Image
  and Calculations on plates and channels; channel Levels and scalar histograms; Gamut Warning; divisor regions (strided
  `CmykStack::row_strided`, the styled-row kernel included); the kept staged plan is its largest stage; Divide and Vivid
  Light's edges agree with Photoshop's at every depth (`blend-edge-547/take1`, Hard Mix MISS); no DRDC change.
- **Blend If** (2026-10-15): Gray and the four plates on CMYK rasters and isolated groups — the gate, the Layer Style
  sheet (split sliders), MCP `set_layer_style`, Mode/Profile/Depth conversions, `.rz` and CMYK PSD import and export.
  Fitted on take1 (`--fit`), confirmed by take2 (`cmyk-styles-547/take2`, every channel BUILT). No DRDC word: Blend If
  joined word 86's claim, `LBIF` codes 4–7 already existed.
- **Hard Mix's exact tie** (2026-10-15): `blend.rs`'s `b_hard_mix` breaks `L_b + L_s = 1` by order, 1 where
  `L_b − L_s > 1/32768` (so 16-bit 32767 / 32768 reads 0), shared by every RGB depth and `ink_blended`; confirmed by
  `gray_luminance_take.py` take1 at RGB 8, RGB 16 and CMYK (every tie patch exact). No DRDC change.
- **Photoshop opens styled CMYK files** (2026-10-15): `grdm` is written in Photoshop's form — padded to a multiple
  of 4, its two opaque transparency stops and seed. `cmyk-psd-open-662/take1` blamed only `grdm` (PSD/PSB refused, TIFF
  flat); take2 opened every construct and the full styled export with every layer (PSD, PSB, layered TIFF; RGB too).
- **Pattern Overlay in ink** (2026-10-15): the effect on CMYK at 8 and 16 bits, in the sheet, MCP, `.rz` (word 90
  after the rung) and CMYK PSD (RGB pattern records); `cmyk_pattern_overlay.rs` against the Colour Overlay oracle.
- **RGB → CMYK against Photoshop: the engine residual is accepted** (2026-10-22). `rgb-cmyk-684/take1`
  (`rgb_cmyk_take.py`; sRGB, 39 patches, U.S. Web Coated (SWOP) v2, NA General Purpose 2, 1-code gate) MISSED:
  today's Relative + BPC route is closest at max / mean 6.75 / 0.98 codes (8 bits) and 6.95 / 0.98 (16); every
  other intent, BPC or dither candidate is worse. Photoshop's cyan runs high, most in near-black shadows: ACE against
  our CMS. The owner accepted it; `cmyk_color.rs` is unchanged and `core/tests/cmyk_mode_photoshop.rs` pins the
  residual and Lab 5b's 6.06 / 5.88 re-measure.
- **Auto-Align and Reduce Noise from Stack** (2026-10-23): registration on each layer's Display P3 view (its ink
  through the profile), every ink warped, the statistic per ink; Vignette Removal and Geometric Distortion refuse by
  the core's sentences. `cmyk_align_stack.rs`, `verify_cmyk_mode.py` G9. No DRDC change.
