# High-depth operation support matrix (ADR)

- **Status:** Accepted — open items remain ([#2](https://github.com/kpgalligan/doctored/issues/2), [#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11), [#31](https://github.com/kpgalligan/doctored/issues/31)) (the matrix below is the live summary; its float column, masks and RAW rows re-verified against code 2026-10-30)
- **Phase / dates:** 7A audit → 7E public matrix, 2026-09-10; amended through Phases 8–12
- **Original:** [archive/phase-7-operation-support.md](../archive/phase-7-operation-support.md) — full history, the original RGBA8 boundary audit, per-slice notes
- **Key commits:** `082ccb1` (audit), `3bd4349` (public gate); later rows `e8af30c` (12E-2c), `07d9153` (12F-2)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Every existing operation had an RGBA8 contract, and many lost precision in ways a wider store would not fix (import
`to_rgba8()`, isolated-group `quantize`, byte Curves LUTs, the byte `ToneEncoder`, byte-tolerance profile equality,
byte mask/alpha rounding, `to_rgba8()` in native reads). The 7A audit listed each boundary and the migration it
needed; 7E turned the qualified subset into one public operation/target/depth gate. The
[carryover ledger](phase-7-carryovers.md), closed 2026-10-30, recorded every remaining gap, and its open rows are
issues; this matrix states only what works today.

## Decisions
- **One gate, core-enforced.** The host gate is `DocumentPrecision.refusal` (`app/Sources/DocumentPrecision.swift`:
  `commonTools`, `typedTools`, `rasterTools`, `coreRefusal`) for 16-bit and `FloatDocument.tools`
  (`app/Sources/FloatDocument.swift`) + `DocumentPrecision.floatCoreRefusal` for 32-bit float; both map tools onto the core family table
  in `core/src/precision_capability.rs` (`FAMILIES`, `FLOAT_FAMILIES`). UI menus (`validatePrecisionItem`), MCP
  (`AgentServer` before dispatch) and Actions replay share it. Unknown operations/targets/depths fail closed; a
  supported family still passes strict parameter, record-kind, profile and resource checks.
- **Refuse by name, never fall back.** Legacy byte APIs never receive a typed pointer; a refused tool says why and
  points at explicit conversion. Refusal happens before mutation, scheduling or recording.
- **Intentional depth changes pass one central conversion** ([conversion/readout](phase-7-conversion-readout.md)).
- **Byte inputs keep their declared precision** (legacy geometric/brush inputs, Vision outputs, glyph assets,
  byte-valued Action arguments); promotion to u16 is ×257 and disclosed.

### Current matrix (16-bit = `rgba16_unorm`, float = 32-bit linear extended range)

| Family | 16-bit | 32-bit float | Refused / limits |
| --- | --- | --- | --- |
| Import / mode | RGB(A)16 + Gray16 PNG/TIFF, 16-bit PSD, RAW (retained source), typed native `.doctored` (legacy `.rz` read); `get_precision`/`convert_precision` | Float TIFF, OpenEXR (`exr_float.rs`), Radiance `.hdr`, 32-bit PSD/PSB, HDR host stills (`float_host_intake.rs`), the 32-bit float RAW development, float `.doctored` | Typed import never uses ImageIO byte fallback |
| Conversion | Flattened or `structure:"preserve"` (12H), none/ordered dither to 8-bit; 8/16 → 32-bit float, each code the profile's TRC decode (`precision_conversion_float.rs`) | Tone-mapped to 16 or 8 | Alpha never dithered; promotion adds no precision |
| Structure | Duplicate, name/visible/label, reorder/group/ungroup/delete, merges, **Stamp Visible / Flatten Image** (12E-2c), locks/links, comps, guides/ruler, `set_resolution`, new layer, group open/close | Same set | Same |
| Blending | 27 modes on raster, adjustment and isolated groups; Pass Through (groups); clipping; Blend If and layer styles on raster/groups; knockout | Every mode but the nine that invert about white; Pass Through (groups); Blend If, layer styles in the admitted modes, fill, knockout, Blend Interior and the global light on raster/groups ([v1 float editing](v1-float-editing.md) rule B) | no Blend If/style on adjustment layers; styled/Blend-If group refuses Pass Through; float: Screen, Overlay, Soft Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix and Exclusion refuse by name |
| Tone / profiles | Live adjustments at a chosen slot; selected destructive tone on RGB and scalar targets; Auto Tone/Contrast/Color; assign/convert profile | Adjustment layers and destructive apply under the float rules ([v1 float editing](v1-float-editing.md) rule A); assign/convert profile | Float: Invert, Shadows/Highlights, the legacy White Balance and Auto Tone/Contrast/Color refuse by name; Assign Profile relabels the linear samples and Convert to Profile applies the matrix between the two profiles' primaries with no clamp (a profile with no matrix/TRC model, absolute intent, black-point compensation and preserve-appearance refuse by name) |
| Spatial filters | Every spatial filter the app has at 8 bits (selection-aware): Blur, Sharpen, Unsharp Mask, High Pass, Reduce Noise, Dust & Scratches, Edge Detect, Emboss, Pixelate, Add Noise, Box Blur, Surface Blur, Add Grain, Radial Blur, Clouds, Difference Clouds, Offset, Motion Blur, Lens Blur, Tilt-Shift | The Filter menu in linear light or on an encoded copy, per filter, on layers and on masks, alpha channels and colour planes ([v1 float editing](v1-float-editing.md), "Filters") | No `apply_filter` name or Filters-menu item is refused at 16 bits; each spatial filter is held to its 8-bit twin in `../core/tests/filter_depth_parity.rs`. 16-bit `blur` takes sigma up to 64 (8-bit: 1000). Float: Liquify, Depth Blur and the Develop Filter refuse on a mask, channel or plane; Develop Filter masks refuse on a float layer (`develop_filter_masks.rs`) |
| Geometry | Resize, crop, Canvas Size, straighten, arbitrary rotate, quarter turns/flips, numeric/drag affine+perspective on leaves, masks, sets, groups and adjustments, warp; Move drag/nudge; Move selected pixels; Free Transform of selected pixels; Move Auto-select, `align_layers`, `distribute_layers` | Canvas Size, zero-angle crop, quarter turns/flips, exact integer Move; filtered geometry (resize, straighten, arbitrary rotate, affine/perspective, warp) with each layer's RGB clamped to its own visible range; Move selected pixels and Free Transform of selected pixels on a layer (filtered landings clamped to the floated pixels' range) and on a mask or alpha channel; Move Auto-select, `align_layers`, `distribute_layers` | Selected pixels of a colour channel refused at every depth |
| Masks / channels / selections | u16 masks (link, density, feather, outside), channels, Apply Image/Calculations, luminosity masks, source-derived selections, Quick Mask, Select and Mask (Refine Edge brush, decontamination, New Layer outputs) | Mask/channel CRUD and properties, mask transforms, selections, Save/Load Selection; Apply Layer Mask, Calculations and luminosity masks; Select and Mask without Decontaminate | Float: Apply Image and Decontaminate Colors refuse by name |
| Paint / retouch | 27 paint modes, erase, fill, gradients, dodge/burn, clone/heal with Current/Below/All sampling, spot heal, red-eye, content-aware fill, Patch | Brush, Eraser, Gradient and Clone Stamp in linear light; Paint Bucket, Dodge/Burn and Sponge (v1 rules R3–R5); Fill…/Stroke… on layers, masks, alpha channels and colour planes; heal, spot heal, Patch, Remove, Content-Aware Fill and Move in the working encoding; Blur, Sharpen, Smudge and History brushes ([v1 float editing](v1-float-editing.md)) | Brush, Paint Bucket, Fill…/Stroke…, Gradient and Clone Stamp take all 27 modes at 8/16 bits; at float, ink refuses the nine modes the Blending row names (a clone onto a new layer and a gradient on a mask, plane, channel or the Quick Mask are Normal only); Healing, Spot Healing, Dodge/Burn and the Eraser are Normal only (a heal replaces the covered pixels with a solved membrane, so it has no source colour to blend); 50,000-dab and footprint caps |
| Artwork | Retained text/shapes (`add_text_layer`, `add_shape_layer`) | Text and shape layers, Artwork Colors, Rasterize Artwork; colours are linear-light numbers | Legacy `add_text` byte tool refused; 4,096 UTF-16 units |
| RAW layers | Place Embedded/Linked… of a camera file, Redevelop RAW…, RAW Source…, Rasterize RAW Layer | Same; a 16-bit development renders into linear light, and the 32-bit float development keeps an f32 master (`float_native`) | Destructive pixel edits need Layer ▸ Rasterize RAW Layer first (`RetainedImagePolicy`) |
| Clipboard | Cut/Copy/Copy Merged/Paste, New from Clipboard; Layer Via Copy / Via Cut (`layer_via_copy`/`layer_via_cut`) | Cut/Copy/Copy Merged/Paste, New from Clipboard; Layer Via Copy / Via Cut | Layer Via rasterizes at every depth: the new layer keeps no description or style |
| Readouts | Native sample sums/means; histograms with exact endpoint counts; selection/backdrop scopes | Sample, histogram (scalar: masks/channels) | 256 display bins are not clipping counts |
| Output | Native save, PNG/TIFF16 from captured projection, export workspace, layer export, PSD; soft proof of the display codes, `render_proof exact` for the full composite | Native save, export | `export_pdf` writes an 8- or 16-bit page; soft proof refused at 32-bit float |
| Evaluation policy | `encoded` / `linear_light` (11E) | Holds linear light by definition | Under `linear_light` every operator evaluates; storage edits (paint, fill, filters, mesh warps) still write stored codes ([11E](phase-11e-linear-light-status.md) D9) |

## Rejected alternatives
- Per-surface capability lists in UI, MCP and Actions separately — they drift; one host gate over one core table.
- Silent byte fallback for unsupported typed operations — would reduce precision without consent.
- Labelling a format high-depth because its container can hold wide samples.

## Consequences
- A new typed tool is enabled only by adding its family to `precision_capability.rs` and its tool name to the host gate
  (and to `validatePrecisionItem` for menus); anything absent is refused at that depth.
- The open issues, the implementation plan and this matrix must stay consistent when rows move.

## Open items (verified 2026-09-26)
- `DONE` — `arrange_layer` is admitted at 16-bit and float (`typedTools`, `FloatDocument.tools`, both `families` maps
  → `layer_move`), and a recorded Arrange replays through `run_action`. `AgentServer+Groups.arrangeLayer` answers the
  no-op from `LayerTree.canArrange` and fails loudly on a core nil. It is verified by
  `scripts/verify_phase12e1_structure.py` S6.
- `DONE` — the five typed structure wrappers (`precisionMoveLayerTo`, `precisionArrangeLayer`,
  `precisionRemoveLayers`, `precisionUngroupLayer`, `precisionGroupLayers`) and their six `DoctoredCore` facades throw
  the core's sentence; `editStructure` rethrows it, so MCP answers `isError` and a replayed Action halts on it, and
  the UI presents the sentence (`applyEdit` catches the thrown refusal). `delete_layer`, `ungroup_layers` and
  `group_layers` decide their no-op / cross-parent refusal up front. The typed colour label runs through
  `applyingPrecisionBookkeepingEdit` (synchronous, no `BackgroundWork`) and `applyEdit`'s `renderNeutral` carry keeps
  the current projection, so it opens no modal panel, refuses during background work and stays one undo step.
- `DONE` — `DocumentPrecision.isDocumentFree` admits `save_raw_preset` given authored settings at every depth, and the 17 tools whose handlers read no document moved from `commonTools` /
  `FloatDocument.tools` into `documentFreeTools`. The remaining typed layer/channel facades (add, merge ×3, stamp,
  flatten, lock, link, unlink, name, visible, clipped and the five channel edits) throw the core's sentence; UI call
  sites use `try` and `applyEdit` presents it instead of beeping, and the Layers panel drop presents a thrown reorder
  refusal. `merge_visible`, `stamp_visible` and `duplicate_channel` decide their no-op up front, because the typed
  core refuses where the byte core answers nil. The typed rename runs through `applyingPrecisionBookkeepingEdit`, and
  the panel's inline rename passes `renderNeutral`; the group open/closed half landed separately.
- `DONE` — typed lock, link, unlink and channel name/overlay run through `applyingPrecisionBookkeepingEdit` with
  `renderNeutral` at every call site, and a name-only `set_layer_properties` at depth publishes render-neutral.
  Opacity, blend mode, duplicate, the move/transform sets, whole-document geometry, per-layer transform/perspective,
  the mask edits, `addingChannel` and the three profile facades throw the core's sentence; `add_channel`, the
  new-channel `save_selection` and `calculations` decide a full channel list up front. Thirteen more document-free
  tools (`new_document`, the clipboard read, New from Clipboard, the action library/recorder, batch and bundle tools)
  moved into `documentFreeTools`. `dr_precision_doc_from_rgba16` takes an optional layer name, so a 16-bit New
  Document names its layer at construction and needs no rename edit. `applyingDocumentGeometry` throws, a thrown
  described-transform refusal is presented (UI) or answered as a `ToolError` (MCP), and the shape tools use the
  throwing artwork functions. History records `renderNeutral` per transition and Undo/Redo of such a transition
  carries the current projection.
- `DONE` — `withLayerOffset`, `removingLayerMask`, `cropped`, `canvasResized`, `settingMetadata` and
  `settingResolution` throw the core's sentence instead of swallowing it with `try?` (byte branches unchanged, nil
  the exact no-op; the typed `withLayerOffset` answers nil up front for an unchanged offset). A refused Shape
  re-edit keeps its session open like the Type tool's: `commitShapeEditSession` answers false and a tool switch,
  Quick Mask, Free Transform and the adjustment/style/ host-frame editors stop on it, while a text-layer re-edit
  only commits the open shape session. Free Transform commits (or stops on) an open text or shape session before
  any hand-off, at every depth.
- `DONE` — `commitPendingSessions` and `commitPendingCanvasSessions` answer false when a refused text, shape or Free
  Transform commit leaves a session open, and Save, Save As, close, close-time Save, Print, both exports and the
  fifteen feature entry points (plus a tool switch on a refused Free Transform) stop on it. The typed facades name
  their refusals: Crop and Canvas Size throw when an artwork description cannot be read, Canvas Size throws the
  100-megapixel and non-positive-size sentences at both depths, an out-of-bounds 8-bit Crop throws, the guide/ruler
  mutators rethrow the facade capture error, and `dr_precision_doc_set_metadata` / `dr_precision_doc_set_resolution`
  gained `err_out`. The byte pair `dr_doc_set_metadata` / `dr_doc_set_resolution` gained the same `err_out` and the
  same sentences. The byte colour trio followed.
- `DONE` — the MCP writers (`save_copy`, `export_image`, `quick_export`, `export_psd`, `export_pdf`, `export_layers`,
  `contact_sheet`, `package_image_sources`) commit an open canvas session first, as File ▸ Save / Export do, in its
  own Undo step (`AgentServer+PendingSessions.swift`); a refused commit answers with the refusal's own sentence,
  captured by `SessionCommitRefusal.swift`. `contact_sheet_from_sources` never commits its read-only sources and
  refuses by name while one has an open session. `render_proof`, `get_evaluation`'s `preview_refusal` and
  `get_proof_setup.failure` carry the core's render refusal (`render()`, not `flattened()`).
- `DONE` — the 8-bit composite readers read the retained projection when it is the handle's own and otherwise
  `render()` (`ImageDocument.byteComposite(of:)`): Apply Image / Calculations from Merged and their sheet previews,
  the Evaluation sheet and `get_evaluation` readout's `before`, and Remove Red Eye / `red_eye_auto` (which at 16/32
  bits now reads `currentDisplayProjection()` instead of always failing to flatten). A render failure answers the
  core's sentence — an alert in the UI, `isError` over MCP — instead of `changed:false` or "could not read". The
  dead `HistogramSource.image(in:)`, `Histogram.of(_ document:)`, the Info panel's `resolvedImage` memo and
  `RasterDocument.subjectMask` are deleted. The remaining readers (`PixelReadout`, `copy_merged`, Print, the
  eyedropper, `SelectionCapture`'s stored-error rule, the typed `projectionDifference`, the scoped analysis
  composite) are the next bullet.
- `DONE` — the remaining 8-bit composite readers read `byteComposite(of:)` and answer a render failure with the core's
  sentence: the Info readout and `sample_pixel` (`PixelReadout.at` throws; the panel shows "—" and names the failure
  in its footnote), Copy Merged (`mergedCopySource()`: one alert in the UI, `isError` from `copy_merged`), the
  eyedropper (`eyedropperComposite()`: never an alert, a beep on the press only) and `sample_color`, Print (which at
  16/32 bits now prints the display projection instead of "There is nothing to print.") and `SelectionCapture`'s byte
  source (no longer the stored `projectionError`). The typed `projectionDifference` reads the retained typed
  projection `get_evaluation` hands it. The scoped analysis composite reads the retained typed projection (see the
  16-bit composite-scope bullet below). A canvas-session commit refused by a beep, a declined rasterize prompt or a
  cancelled transform job answers the MCP writer with that reason (`SessionCommitRefusal.beep`), and a Free Transform
  of a described layer whose publication `applyEdit` refused keeps its session instead of reporting success, and so
  does the byte selected-pixels Free Transform.
- `DONE` — the high-depth readers stop discarding read errors. `PrecisionReadout.at` / `FloatReadout.at` answer nil
  off the canvas (checked before any render) and throw everything else; the eyedropper beeps on the press at every
  depth, the Info readout and scoped cursor show "—" and name the reason in the footnote at both depths, and the Info
  histogram reads "Histogram unavailable" with the sentence in its tooltip and the footnote
  (`HistogramLoader.onFailure`; a cancel, mid-run or not, and a superseded request are never shown as a failure). The
  typed Evaluation sheet measures against the retained typed projection instead of rendering "before" again. The
  scoped analysis over the retained projection and float Info analysis are resolved below; the float eyedropper is
  resolved: it picks the float block mean into the foreground, above white kept ([11F ADR](phase-11f-hdr-float-status.md)). The
  Levels/Curves sheet histogram is resolved: the sheets show "Histogram unavailable" with the reason as the tooltip.
- `DONE` — at 16 bits the composite scope of `ScopedAnalysisRead` (Info histogram, scoped cursor, MCP `histogram` /
  `sample_pixel`) counts the retained typed projection through `dr_precision_image_scoped_analysis{,_plan}` (composite
  only; layer/backdrop refuse by name) instead of rendering again, with answers equal to the document path's. At
  32-bit float the Info panel shows the stop-binned histogram ("Below zero · above white") and the linear readout. The
  dead u16 `dr_precision_image_histogram` chain is deleted.
- `DONE` — the float scoped analysis: the core's `scoped_analysis` dispatches f32 storage to `float_stats::evaluate`
  (stop bins, below-zero/above-white counts and weights, weighted linear sample), so at 32 bits the Info histogram and
  cursor and MCP `histogram` / `sample_pixel` / `sample_color` answer Composite, Active layer and Below active layer
  with selection weighting, the composite over the retained float projection. `dr_precision_image_float_histogram` and
  `FloatDocument.infoSourceRefusal` are deleted; MCP `histogram source:"scalar"` at 32 bits has since been resolved
  (masks and channels measure; red/green/blue/alpha refuse in the core).
- `DONE` — the 8-bit composite scope of `ScopedAnalysisRead` (the same four readers) counts `retainedByteProjection`
  through `dr_doc_projection_scoped_analysis{,_plan}` (composite only; a projection that is not canvas-sized refuses)
  instead of rendering on every scan, with answers equal to the document path's. The Levels/Curves sheet histogram of
  a new adjustment layer landing above the whole stack now plans `.composite` and counts the retained projection at
  both depths, the 16-bit Color Range sample reads the retained typed projection, and the 8-bit Color Range already
  read `byteComposite(of:)`; the `.edit` and partial-backdrop sheet plans still render their backdrop.
- `DONE` — typed `align_layers`/`distribute_layers` at 16 bits and float (PR-B: the core's
  `align_set`/`distribute_set`, one typed edit; `core/src/precision_align.rs`); Auto-select landed (PR-A).
- `DONE` — Stamp Visible / Flatten Image at depth (the archived row still said unavailable): `e8af30c`, families
  `layer_stamp`/`layer_flatten`.
- `DONE` — knockout at both depths (carryover P7-37, verified 12E-3…12E-3d) and `blend_interior_as_group` on a clip
  base with visible members (P7-39, verified); `lfx2` PSD exchange (P7-21).
- `DONE` — typed clipboard (P7-29 closed, 12G); group/adjustment/linked-set Free Transform and retained-source
  geometry (12F-2, `07d9153`).
- `DONE` — typed global light and native group open state.
- `DONE` — advanced cutout refinement: edge brushing and colour decontamination ship in Select and Mask (2026-10-11;
  P7-12).
- `DONE` — linear-light phases 2–3 (P7-24, 2026-11-12).
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — full-frame projection fallback and stage-only
  cancellation (P7-31).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — payload compression and codec cancellation (P7-32);
  large-document workloads and the broader camera corpus (P7-33); dab/text/plane caps (P7-34).
- `DONE` at RGB 8/16 — a Background layer kind (P7-38): the Background mark, set by PSD import, written by PSD
  export, `.doctored` word 113. `OPEN` [#31](https://github.com/kpgalligan/doctored/issues/31) — the same in the
  Gray, Lab, CMYK and 32-bit float PSD codecs.
- `OPEN` [#2](https://github.com/kpgalligan/doctored/issues/2) — `blend_interior_as_group` on a clip base below 100 %
  Fill.
