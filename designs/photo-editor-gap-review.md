# From compositor to photo editor: the first gap review (ADR)

- **Status:** Accepted — live inventory (rows re-verified 2026-10-30). Prioritisation superseded by [photo-editor-gap-review-2026-09.md](photo-editor-gap-review-2026-09.md) and the [implementation plan](professional-photo-editor-implementation-plan.md); the gap catalog below is part of the feature inventory, and a row whose work is open links its issue.
- **Phase / dates:** gap-review phases 1–8, early September 2026 (last update 8 Sep 2026)
- **Original:** [archive/photo-editor-gap-review.md](../archive/photo-editor-gap-review.md) — full history, measurements, receipts
- **Key commits:** `125795a` (1 styles), `70000ac` (2 symbolic transforms), `e860fe2` (3 channels), `3d22373` (4 colour/metadata), `c4f38be` (5 adjustments), `315d98c` (6 healing/CAF), `dbe8382` (7a groups/locks), `25da0fd` (7b guides/snapping), `93af0d8` (8 RAW + Actions)

## Context
A review of the shipped compositor against a photographer's everyday work. It found three problems: colour
was not managed, parametric layers could not be transformed, and the stack had no groups, styles or channels. It
then sized a catalog of breadth (§3) and put it in order (§4). Steps 1–8 of that order shipped one phase per commit.
The September review then reordered what remained around integrity and precision.

## Decisions
**Cross-cutting**
- Every feature ships on three surfaces: UI, an MCP tool whose catalog entry names the UI path, and the native `.doctored` file where state persists. It also gets core tests (the recipe in the root `CLAUDE.md`).
- Per-layer render caches (style planes, the content box) are keyed on `Arc` pointer identity plus parameters. This key is exact only because no op mutates an `Arc` buffer in place.
- The `.rz` bumps named here (v4 styles, v5 channels, v6 profile/metadata tail, v7 groups, v8 guides) are historical. The format has gone well past them, but every one of them still loads.

**Styles (phase 1)**
- `Layer.style: Option<Arc<LayerStyle>>` is a core field, not `meta`. Its JSON schema lives in `core/src/style.rs` and is the FFI and native-file contract. Effects render from the layer's shape (alpha × enabled mask) in a fixed order. Layer opacity applies once to the whole package. A byte (8-bit RGB) adjustment layer's style takes its shape from the layer's mask (`style_adjustment.rs`).

**Symbolic transforms (phase 2)**
- Text and shape payloads v2 (and the Live Photo payload, until its removal on 2026-10-10) carry `transform [a,b,c,d]` and `origin_frac`. Translation always comes from the core's layer offset, never from the payload (the anchor rule is in `app/Sources/DescribedLayer.swift`). A true perspective quad still asks to rasterize a described layer.

**Channels (phase 3)**
- `DrDocument.channels` holds canvas-sized planes, each with a stable id; the UI keys on the id, not the index. Channels have their own budget, `MAX_DRDC_TOTAL_CHANNEL_PIXELS` (9 × `MAX_PIXELS`).
- `doc_plane.rs` is the one plane implementation. Plane edits run in the layer's own space and write back through `doc_scope::apply_layer_plane`. The edit target is `PaintTarget`: layer | mask | plane | channel.

**Colour and metadata (phase 4)**
- The ICC engine is ours (`icc.rs`, `icc_transform.rs`, `icc_builtin.rs`), not `moxcms`. We must write profiles byte-exactly and guarantee no input panics. There is no curve inverter: encoding binary-searches forward-evaluated thresholds.
- The PCS white comes from the r/g/bXYZ tags. `chad` and `wtpt` are parsed and not used, because Apple's own sRGB profile proves they are advisory.
- A document profile is never absent (the default is built-in sRGB). An RGB document refuses Gray/CMYK/Lab profiles and non-space profile classes (device link, abstract, named colour) with a sentence. A CMYK document keeps its CMYK profile ([status](cmyk-mode-status.md)) and a Grayscale document its Gray profile, viewed through an RGB twin ([ADR](grayscale-mode.md)). The other document modes, [Lab Color](lab-mode.md), [Indexed Color](indexed-mode.md), [Duotone](duotone-mode.md) and [Bitmap](bitmap-mode.md), are recorded in their ADRs; the colour modes' remaining open items are [#1](https://github.com/kpgalligan/doctored/issues/1) and, for Lab, [#5](https://github.com/kpgalligan/doctored/issues/5).
- Open rule: assign the profile the numbers belong to, then adopt the working space once, for every open of a file that is not native (`.doctored`, or the legacy `.rz`).
- `ColorProfile` answers two questions: the tag, and the space to draw into. Authored colours (colour well, MCP hex, style colours) are sRGB and convert where they are used. Sampled colours are never converted, so sample → paint is byte-exact.
- EXIF is patched in place and never grown: Orientation → 1, and ppi and dimensions are rewritten. An absent tag stays absent, because inserting one corrupts MakerNotes. Stale Photoshop APP13 blocks (`0x03ED`, `0x040F`, `0x0422`, `0x0424`) are filtered out. ExtendedXMP is read (merged into one logical packet on open) and written (an oversize packet is split on JPEG export); flat exports drop Google's `GImage:`/`GDepth:` properties.

**Adjustments (phase 5)**
- There is one destructive twin, `dr_image_adjust_op` (JSON params + `err_out`), running the same `Adjustment` code as the compositor. At full strength, `composite_adjustment_into` takes the adjusted value verbatim (`k >= 1`), which is what keeps layer and filter byte-identical. `adjustment_tests::full_strength_adjustment_layer_is_byte_identical_over_every_level` pins this.
- There are three stated exceptions to that byte identity. `shadows_highlights` is spatial: its guide plane comes from the backdrop for the layer and from the image for the filter. `gradient_map` dither is keyed by position. The legacy nine ops keep their 0–255 arithmetic.
- Adjustment params are document-space numbers and are never converted through a profile. Named presets are authored sRGB and convert once, when the user picks one.
- One gamma convention: `levels` and `exposure` both brighten above 1 (`exposure` applies the reciprocal power, measured against Photoshop's Gamma Correction).
- A grey has no hue, so no Hue/Sat band reaches it; only master lightness moves it (`adjust_hue_saturation.rs`).
- A LUT is stored in meta, capped at 33³ for 3D and 1024 for 1D; larger `.cube` files are resampled. A 16-entry LRU memo applies only when the meta is over 64 KiB.
- Color Lookup also loads ICC looks (`icc_lookup.rs`): an RGB→RGB device link is sampled into the same 33³ table, and an abstract profile (Lab or XYZ on either side) is baked through the document profile (relative colorimetric), keeps its bytes in the layer and is re-baked through the new profile on every Assign, Convert and Don't-Convert paste (native-file word 71). Every other class or space is refused by name.
- Auto Tone/Contrast/Color are commands, never layers. They clip 0.1% per end and use a neutral-candidate mean (not gray-world), with gamma clamped to [0.5, 2].
- The histogram counts a pixel only if alpha > 0 (and mask coverage ≥ 128).
- Photoshop preset files (`adjustment_preset_file.rs`) add no codec: `.alv`/`.ahu`/`.eap`/`.blw`/`.cha` ARE the `levl`/`hue2`/`expA`/`blwh`/`mixr` record bodies and go through `psd_adjustment::decode` verbatim, and `.acv` is reframed onto `curv` (four RGB curves at most; a non-identity fifth — a CMYK preset — refuses). The preset only prefills the create dialog, whose Apply records the resolved `add_adjustment_layer`, so a preset's name or path never reaches an Action. There is deliberately no destructive twin of the preset command yet.

**Retouching (phase 6)**
- `poisson.rs` is the one membrane solver: red-black multigrid on a fixed schedule, with no convergence test. `patchmatch*.rs` is the one PatchMatch. Healing, Spot Healing, Patch and Content-Aware Fill (CAF) are compositions of the two.
- Healing overlays use the one `dr_doc_painting_layer` premultiplied overlay. Brush heals cut hard at α = 128, and Flow is forced to 1. CAF instead ramps from coverage 0 (`Caller::hole_threshold`).
- Caps bound one call (`MAX_INPAINT_*`, `MAX_SOLVE_WINDOW_PIXELS`) and are refused from the bounding boxes, before any allocation. Agent tool calls hold `AppActivity.userInitiated`, because App Nap otherwise slows heavy ops about 5×.
- Red-eye scores red dominance, not R > G. A component touching all four sides of the rectangle is rejected.

**Groups and locks (phase 7a)**
- The stack stays a flat `Vec<Layer>` with `kind` + `depth`, so no FFI signature changed. The invariant is `doc_group::validate_structure`, a hard runtime check (not a `debug_assert!`) run at both trust boundaries and on every structural op's result.
- A group renders to a synthetic layer. Pass Through is the cheap case. A pass-through group's mask or opacity gates it (`composite_gated_pass`) and does not isolate it.
- Clipping forces isolation, and this is a published cost (`group_tests::clipping_over_a_pass_through_group_isolates_it`). `MAX_GROUP_DEPTH` 10 bounds recursion and `MAX_GROUP_BUFFER_PIXELS` bounds allocation.
- The transparency lock freezes the alpha channel. A transform counts as a position edit. `under_locks` is the one enforcement point.
- Preserve Transparency and Lock Transparency on paint: an alpha-0 pixel stays as it is, and a semi-transparent one takes `lerp(Cb, B(Cb, Cs), sa)` and keeps its alpha, so the paint mixes in at its own opacity × coverage instead of the W3C blend's colour being boosted by the alpha restore (a Normal red fill at 0.6 on (0,0,255,128) was (191,0,64,128) and is now (153,0,102,128)). The rule is `blend::composite_source_frozen`, used by the byte walk, bucket and gradient, the 16-bit native paint, brush and in-place clone, and the float fill. `under_locks`' alpha restore still runs afterwards and is now a backstop for paint; plane, mask and channel painting, filters, heal and dodge/burn keep restore-only semantics.
- Byte paint is ONE walk, `doc_paint::paint_walk`, over an ink closure: the brush, eraser and Clone Stamp pass their premultiplied overlay, while Fill… / Stroke… pass a straight colour at the selection's coverage and build no canvas-sized overlay. A non-Normal fill, and the colour brush's blend modes (`dr_doc_painting_layer_blend_color`, the straight ink with the overlay's alpha as coverage), blend the exact colour: Multiply (200,100,50) at coverage 3 on a transparent pixel was (170,85,85,3) and is now (200,100,50,3). Normal and Erase stay byte-identical, and Clone Stamp keeps `dr_doc_painting_layer_blend`. Peak memory of a whole-canvas Normal fill on a 60 MP opaque layer (`core/tests/paint_walk.rs`'s ignored probes, fill minus baseline maximum RSS after a `--no-run` build) fell from 721,141,760 to 481,116,160 bytes: 240 MB lower, the overlay's 10000 × 6000 × 4 bytes.
- ⌘J routes to Duplicate for any layer that is not a plain raster, so it never silently rasterizes one. `get_document` stays flat and index-addressed, with additive keys. Tools that act on a set of layers take explicit arrays.

**Guides and snapping (phase 7b)**
- Guides and the ruler origin are document state (`f64`, tail-appended in v8, `MAX_GUIDES` 1024 via `dr_max_guides`). Rulers, grid, units, snap toggles and Lock Guides are app preferences, the agent-parity exemption in `app/CLAUDE.md`. Guides and the ruler origin have MCP tools (`list_guides`, `add_guide`, `remove_guide`, `clear_guides`, `set_ruler_origin`); there is no `move_guide`.
- One solver, `SnapEngine.pull`. The pull is 8 screen points and ⌃ suspends it. Ties resolve guides > layers > selection > bounds > grid. Snapping does not round to whole pixels unless the site opts in. Crop drops out-of-window guides.
- A guide is grabbed with Move, or with ⌘ from any tool (`canGrabGuide`). Guide edits go through `applyGuideEdit`, which does not reproject.

**Actions and RAW (phase 8)**
- The recorder is an explicit `record:` parameter with no default on the commit entry points, and its vocabulary is `ActionSteps.swift`. A step that cannot be recorded becomes a visible `__unrecorded` placeholder, never a silent gap.
- Symbols are `$layer`, `$layers` and `$guide` only, and geometry is stored absolute (see `Action.swift` before adding remapping). `run_action` and Batch replay as one undo entry (`withReplayUndo`); an interactive Play is one entry per step that changed something (`withUndoGroupDiscardingEmpty`). Batch's MCP twin is `run_batch` / `batch_status` / `cancel_batch`.
- A RAW is developed with `CIRAWFilter` in an app-modal Develop window. Knobs start from the file's values, and a headless path serves the agent.

## Rejected alternatives
- A tree-shaped layer stack: it would have changed 57 header signatures and every stored index.
- Promoting `moxcms`: see the ICC engine bullet above.
- Gray-world Auto Color: it neutralises sunsets and forests.
- A caller-armed global recorder slot: refusals return before commit and would leave it armed.
- The UI routing through the MCP handlers: the two halves differ deliberately.

## Consequences
The byte-identity invariant between an adjustment layer and its destructive twin, the flat-plus-depth structure check, and the
authored-vs-sampled colour rule all still bind every new op. Later phases replaced some of these phases' limits: history no
longer has fixed undo levels, and high-depth, float and linear-light modes arrived. The contracts above still describe the
8-bit path.

## Gap catalog (verified 2026-10-30)
The original's §3 catalog plus the phase-level deferrals. A row whose work is open links its issue; a gap
marked "not built" with no issue is inventory, with no owner yet.

| Area | Gap | Status |
|---|---|---|
| Files | HEIC export; PDF/SVG raster import; PDF export | DONE (plan §11D; `Export as PDF…`) |
| Files | AVIF / JPEG XL export | DONE — AVIF and lossless JPEG XL, with OpenEXR and Radiance HDR (`DoctoredCore+Export.swift`) |
| Files | Linear-light compositing | DONE phase 1 (11E); the rest OPEN → [#12](https://github.com/kpgalligan/doctored/issues/12) |
| Files | 16-bit / float pixels | DONE (Phases 7, 11F) |
| Files | EXIF on RAW/HEIC/TIFF paths | DONE (plan §3B, §11D host-decoder packet bridge) |
| Files | ExtendedXMP read/write | DONE — `metadata_xmp_extended.rs` merges on open, `metadata_xmp_split.rs` splits on JPEG export |
| Adjust | Match Color / Equalize; on-canvas targeted adjustment | Targeted Adjustment DONE — a checkbox in Curves and Hue/Saturation drags on the image (`TargetedAdjustment.swift`), with Desaturate (⇧⌘U) and Replace Color (`adjust_replace_color.rs`, `ReplaceColorSheet.swift`); Equalize DONE (Image ▸ Equalize, MCP `equalize`); Match Color not built |
| Adjust | User adjustment-preset library/browser (only built-in presets exist) | DONE — Photoshop preset files (`.acv`/`.alv`/`.ahu`/`.eap`/`.blw`/`.cha`) open into a prefilled create dialog (Layer ▸ New Adjustment Layer ▸ From Photoshop Preset…, `read_photoshop_adjustment_preset`). The user library (`AdjustmentPreset.swift`, `DR_ADJUSTMENT_PRESETS_DIR`) and each adjustment dialog's Preset popup (save, load, rename, delete; `AdjustmentPresetControl.swift`; MCP `list_`/`save_`/`rename_`/`delete_adjustment_preset`) are DONE too |
| Retouch | Smudge/Blur/Sharpen/Sponge brushes; frequency separation | Brushes DONE — Blur, Sharpen, Smudge, Sponge and the History Brush as tools at 8 and 16 bits (`retouch_brush.rs`, `retouch_smudge.rs`; MCP `blur_stroke`, `sharpen_stroke`, `smudge_stroke`, `sponge_stroke`, `history_brush_stroke`); the Art History Brush DONE — ten Styles, Area, Tolerance and Mode on 8-bit RGB and 8-bit CMYK, refused by name at 16/32 bits (`art_history_stroke.rs`; MCP `art_history_brush_stroke`); frequency separation OPEN → [#17](https://github.com/kpgalligan/doctored/issues/17) |
| Retouch | Remove tool; Content-Aware Move; Content-Aware Fill and Spot Healing above 1 MP | DONE — Remove (`RemoveSession.swift`; MCP `remove_stroke`: the stroke is the hole of one Content-Aware Fill) and Content-Aware Move (`ContentAwareMove.swift`; MCP `content_aware_move`: fill the outlined region, then heal its pixels in at the offset) at 8 and 16 bits; a hole over 1 MP searches at reduced scale instead of being refused (`doc_inpaint::inpaint_lift`), checked on synthetic pictures only |
| Retouch | Liquify (reconstruct/freeze) | DONE — Filters ▸ Liquify…, a workspace with Forward Warp, Reconstruct, Twirl, Pucker, Bloat, Freeze Mask and Thaw Mask at 8 and 16 bits (`liquify.rs`, `LiquifySession.swift`, `LiquifySheetController.swift`; MCP `liquify`, recorded and replayed by Actions); Face-Aware Liquify and saved meshes not planned |
| Retouch | Healing Replace mode | DROPPED — phase 6 (Replace = Clone Stamp); 2026-09 review: only on demonstrated need |
| Retouch | Spot Healing Proximity Match, Create Texture | DONE — the Spot Healing Brush's Type at 8/16/32 bits and on CMYK ink (`spot_heal_type.rs`, `SpotHealType.swift`; MCP `spot_heal_stroke` `type`) |
| Select | Freehand lasso, Color Range, Transform Selection, Reselect | DONE (Phase 6A / 2C; `Tools.swift` `.freehandLasso`, menus) |
| Select | Quick Selection; people instances; Refine Edge | DONE (`.quickSelect` with a sized brush and Enhance Edge and the `.objectSelect` tool; `Select People…` picks individual people numbered left to right and `All People` is one aggregate of every person; `Face Parts…` selects face, skin, eyes, irises, brows, lips or mouth per face; `Sky` selects the sky, `Person Parts…` selects hair, facial skin, body skin or clothes per person, and the Object Selection tool's Object Finder selects an object from a click or box refined by Shift/⌥-clicks, on local models (SAM 2.1, MediaPipe, a sky U²-Net) from Settings ▸ Models; `Select and Mask…` with Edge Detection, the Refine Edge brush, Decontaminate Colors and layer outputs); learned matting (Refine Hair) not planned |
| Select | Magnetic lasso | DONE (`.magneticLasso`, `select_magnetic`, 8/16/32-bit); part B (⌥-drag freehand segments, Caps Lock cursor, contrast keys and stylus pressure) OPEN → [#17](https://github.com/kpgalligan/doctored/issues/17) |
| Select | Stroke Selection… / Fill Selection… dialogs | DONE — Edit ▸ Fill… and Edit ▸ Stroke… at 8, 16 and 32-bit float (`fill_selection`, `stroke_selection`; layer, mask, plane and channel targets; the layer-edge stroke too); at float a layer fills with linear ink in the float layer modes, a colour plane takes the ink's 30/59/11 luma, and a colour above white fills as its own numbers |
| Select | Select ▸ Grow/Similar by colour | DONE (Select ▸ Grow/Similar, `select_similar`; Modify ▸ Expand/Contract) |
| Paint | Multistop gradient in paint tool & style panes; angle/reflected/diamond; presets; midpoints/opacity stops | DONE — the Gradient tool and gradient fill layers take 2–32 stops with midpoints and opacity stops (the shared `GradientStopsPane.swift`), in linear, radial, angle, reflected and diamond; the Layer Style panes (Gradient Overlay, the glow rows) edit two stops, and a gradient preset library is not built: both OPEN → [#18](https://github.com/kpgalligan/doctored/issues/18) |
| Paint | Pattern fill / Define Pattern / Pattern Overlay | DONE — pattern fill layers (Layer ▸ New Fill Layer ▸ Pattern…), Pattern Overlay and pattern strokes (`PatternOverlay.swift`, `PatternOverlayPane.swift`); Pattern Overlay's angle OPEN → [#3](https://github.com/kpgalligan/doctored/issues/3); Define Pattern, a pattern library and Pattern as Fill contents OPEN → [#18](https://github.com/kpgalligan/doctored/issues/18) |
| Paint | Custom/sampled tips, scattering, dual, texture, colour dynamics; Pencil, Background/Magic Eraser | Brush dynamics (pressure curves, tilt and rotation, scattering, texture, dual brush, colour dynamics) and the Pencil, Background Eraser and Magic Eraser OPEN → [#18](https://github.com/kpgalligan/doctored/issues/18); custom/sampled tips not built (tips are parametric) |
| Paint | Swatches/recent colours | Recent colours DONE (`RecentSwatches`, `EditorViewController+Eyedropper.swift`); a Swatches panel with a saved library not built |
| Paint | Foreground/background pair with X / D, ⌥⌫ / ⌘⌫ fill, Lab picker | DONE — X swaps and D resets (canvas keys and rail glyphs); ⌥⌫ / ⌘⌫ fill (⇧ keeps transparency) through Edit ▸ Fill's commit; the Lab picker shipped: a rail swatch click opens the Color Picker sheet (SB field + hue strip, HSB / RGB / Lab / hex, the core's D50 Lab, an out-of-gamut note) |
| Layers | Groups, locks, multi-select, link/align/distribute, ⌘J/Stamp, auto-select | DONE (7a) |
| Layers | Labels, filter, search | DONE (12C-1) |
| Layers | Linked image layers / Replace Contents | REMOVED for v1 on 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)); shipped in 8A/8B (P7-35). A camera RAW layer still places linked |
| Layers | Layer comps; History panel; snapshots | DONE (12D-1; 5C/5D; `levelsOfUndo = 0` memory budget) |
| Layers | PSD `cmls` comps carriage | OPEN → [#13](https://github.com/kpgalligan/doctored/issues/13) |
| Styles | Knockout | DONE (12E-3, P7-37); clip-base interior DONE (P7-39; a clip base below 100 % Fill is unqualified → [#2](https://github.com/kpgalligan/doctored/issues/2)); the Background divergence (a true Background layer kind, P7-38) OPEN → [#13](https://github.com/kpgalligan/doctored/issues/13) |
| Styles | PSD `lfx2` import/export | DONE (10C-2, P7-21) |
| Styles | Expandable effect rows in the layers panel (only an "fx" chip today) | Not built — the Layers panel draws an fx badge (`LayerCellView.swift`) |
| Styles | Pass-through group kept pass-through when it is only a clip base | Not built — clipping isolates the group, a published and tested cost (Groups and locks above) |
| Filters | Unsharp Mask, High Pass, Reduce Noise, median, Dust & Scratches | DONE (9B/9C) |
| Filters | Filter layers | REMOVED — live filter layers on retained sources shipped in 9A/9B and were removed 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)); a backdrop filter layer DROPPED 2026-10-30 (live filters were removed; [plan](professional-photo-editor-implementation-plan.md) Phase 9) |
| Filters | Motion / lens-depth blur, vignette, lens & CA correction on ordinary layers | DONE — Motion Blur, Lens Blur and Tilt-Shift (`sampling_motion.rs`, `sampling_disc.rs`; `apply_filter`); Filters ▸ Neural Filters ▸ Depth Blur… (`depth_blur.rs`; [neural models](neural-models.md)); Filters ▸ Lens Correction…, for distortion, chromatic aberration and vignette (`lens_model.rs`, `sampling_lens.rs`; [Lensfun ADR](v1-lensfun.md)); Filters ▸ Presence… for Texture, Clarity and Dehaze (`sampling_presence.rs`, `sampling_dehaze.rs`); Filters ▸ Develop Filter… with its local masks (`sampling_develop_filter.rs`, `develop_filter_masks.rs`) and RAW Develop's local masks ([8G](phase-8g-raw-enhancements-status.md)); HDR Toning (`hdr_toning.rs`, `hdr_local_adaptation.rs`; [merge and stacks](v1-stacks-status.md)). Open: HDR Toning's built-in preset set → [#7](https://github.com/kpgalligan/doctored/issues/7), `.hdt` import → [#8](https://github.com/kpgalligan/doctored/issues/8); Develop's local tone map and Presence at 32-bit float → [#4](https://github.com/kpgalligan/doctored/issues/4) |
| Filters | Smart Sharpen, radial/zoom/box blur, surface/bilateral blur, film grain, clouds/render, Offset | Box Blur, Surface Blur and Add Grain DONE (destructive, 8/16-bit; [ADR](phase-9f-selected-tone-status.md#additional-spatial-filters)); Radial Blur (Spin/Zoom, Draft/Good/Best), Clouds / Difference Clouds and Offset DONE (destructive, 8/16-bit, selection-bounds frame; [ADR](phase-9f-selected-tone-status.md#radial-blur-clouds--difference-clouds-offset)); Smart Sharpen DONE (destructive, 8/16/32-bit, and an export sharpening method; [ADR](smart-sharpen-status.md)) |
| Geometry | Arbitrary canvas rotation | DONE (12F-1b3 `8e42353`, 12F-1b4 `7f9d55a`) |
| Geometry | Mesh warp | DONE (12F-3, `6400648`, `PersistedWarpSheetController`) |
| Geometry | Puppet Warp | DONE — Edit ▸ Puppet Warp, an on-canvas session of pins on a triangle mesh over the layer (or the selection's float), as-rigid-as-possible, Rigid/Normal/Distort, density, expansion, pin depth and rotation; 8/16/32-bit, one undo step, Actions, MCP `puppet_warp`. Edit ▸ Perspective Warp DONE: Layout quads joined into planes, Warp pins, Shift-click and options-bar straightening, each plane a homography with continuous seams; 8/16/32-bit, one step, Actions, MCP `perspective_warp` |
| Geometry | Trim; canvas Reveal All; perspective crop | DONE — Image ▸ Trim…, Image ▸ Reveal All and the Perspective Crop tool, with MCP `trim`, `reveal_canvas` and `perspective_crop` |
| Geometry | Named skew/distort/perspective modes | Not built — Free Transform distorts by ⌘-dragging a corner; there are no named Skew, Distort or Perspective items |
| Geometry | Auto-straighten (`VNDetectHorizonRequest`) | DONE (Crop box fit, Auto button, Straighten line; MCP `detect_horizon` / `fit_crop`) |
| Geometry | Content-Aware Scale (seam carving) | Not built |
| Geometry | Rotated/scaled on-canvas text editor | DONE |
| Geometry | True perspective on text layers without rasterizing | DROPPED — won't do (the prompt plus Undo covers it) |
| Geometry | True perspective on shape layers staying vector | Not built — a true perspective quad asks to rasterize the shape layer (`describedPrimaryGate`, `EditorViewController+DescribedTransform.swift`); the prompt plus Undo covers it, as for text |
| Exchange | PSD export | DONE (10B/10C); descriptions export as pixels. Private-XMP payload restore DROPPED (10B loss notice; 2026-09 F12) |
| Exchange | PSD group opacity/blend/visibility on import | DONE (10A folder properties) |
| Exchange | PSD layer locks (`lspf`) on import | DONE — `psd_locks.rs` carries `lspf` on import and writes it on export at 8 and 16 bits; inherited folder locks are DONE — a group's locks cover its contents at both depths (`doc_lock.rs`), and refusals name the group |
| Exchange | Export for Web, Quick Export, export layers, contact sheet, batch export | DONE (11A/11B) |
| Exchange | Drag a layer out / Share menu; multipage TIFF/GIF; animation export | Opening multipage TIFF OPEN → [#15](https://github.com/kpgalligan/doctored/issues/15); not built: dragging a layer out of Doctored (layer drag works between Doctored documents), a Share menu, multi-frame GIF and animation export |
| Exchange | Autosave / recovery | DONE (5A/5B, `RecoveryCoordinator.swift`) |
| Actions | Paste, Quick Mask, multi-layer Move, relative move | DONE (12A relative `move_layers`, 12G-2 `$asset` Paste, 2D) |
| Deferred | Bézier paths / vector masks | DONE — Pen tool, Paths panel, Make Selection / Fill Path / Stroke Path / Make Work Path and vector masks at 8, 16 and 32 bits ([ADR](v1-paths.md)); text on a path OPEN → [#19](https://github.com/kpgalligan/doctored/issues/19) |
| Deferred | CMYK / prepress | CMYK mode ([status](cmyk-mode-status.md)) and spot channels ([ADR](spot-channels.md)) DONE, and separations come from CMYK and spot-channel output; PDF/X and the CMYK and spot-channel remaining items OPEN → [#1](https://github.com/kpgalligan/doctored/issues/1) |
| Deferred | Tiled render graph | 13A regional render and the 13B tile store DONE; the canvas display through the regional path OPEN → [#9](https://github.com/kpgalligan/doctored/issues/9), the rest of the tiled backing → [#10](https://github.com/kpgalligan/doctored/issues/10) |
| Verification | On-screen passes listed per phase in the archive (sheets, ⌘P, rulers, Develop, Batch) | DROPPED — superseded by the later phase qualifications (for example 8G camera, 9G mattes, 11C proofing); not tracked individually |
