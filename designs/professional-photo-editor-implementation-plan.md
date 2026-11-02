# Professional Photo Editor Implementation Plan

The roadmap that implements the [September gap review](photo-editor-gap-review-2026-09.md) as thirteen product phases, each split into bounded slices.

**Status (2026-10-30):** Phases 1–12 have shipped within their declared scope; their residual items are GitHub issues. Phase 13 is partly open: 13A (all but 13A-latency) and 30 runs of 13B are implemented, and 13C and 13D have not started. Every open item is an issue under [Open work](#open-work). Smart objects, live filters (9A–9C) and non-RAW retained sources (8A/8B placed and linked images, 8F host media and Live Photo frames) were removed for v1 on 2026-10-10; camera RAW layers stay ([removal ADR](convert-to-smart-object-status.md)). The full plan, with every slice narrative, gate and boundary as written, is archived at [../archive/professional-photo-editor-implementation-plan.md](../archive/professional-photo-editor-implementation-plan.md).

A slice is complete when its outcome works through the UI, MCP, Actions and native persistence (save and reopen, undo and redo, recovery), checked against independent numeric references and, for perceptual quality, real photographs; a model or a menu item alone is not enough. How a slice lands and is verified is in the root `CLAUDE.md`.

## Phases

### 1. Correct saves and renders

Goal: supported work saves, and output renders, without silent omissions. [ADR](phase-1-integrity-status.md).

- **1A — Native capacity contract.** Shared writer/reader limits and an aggregate-pixel preflight; a refused save keeps the previous file.
- **1B — Format-loss detection.** Save routes to the native format when a flat file would drop editable state; flat Export stays separate.
- **1C — Explicit rendering failures.** A group-budget refusal reaches the preview and file output as a failure, never as an empty group.
- **1D — Alpha-correct filtering.** Resize and Gaussian blur sample premultiplied.

### 2. Predictable selection and mask targeting

Goal: a command edits the indicated target and region, and selection work can be undone. [ADR](phase-2-selection-status.md).

- **2A — Operation scope.** One target table for layer pixels, masks, colour planes and saved channels; unsupported combinations refuse.
- **2B — Selection-aware destructive edits.** Filters and adjustments write through the selection's coverage; spatial filters sample beyond it.
- **2C — Selection history.** Undo and redo for selection changes and Quick Mask strokes, and Reselect.
- **2D — Matching edit surfaces.** The same rules in the UI, MCP and Actions, in preview and commit.

### 3. Reliable TIFF, color and metadata handling

Goal: photographic input and output keep their colour and capture information. [ADR](phase-3-color-metadata-status.md).

- **3A — TIFF round trips.** The embedded profile and density are read and the document PPI is written.
- **3B — Capture metadata.** Capture packets survive TIFF and the ImageIO RAW/HEIC paths, with loss reporting per container.
- **3C — Calibrated colour operations.** Exposure and White Balance take the document's colour context under versioned evaluation.
- **3D — PSD compatibility notice.** Known importer limits are disclosed before editing, with a stored-composite option.

### 4. Cancellable jobs and resource foundations

Goal: an expensive edit runs on a worker, commits safely and accounts for its memory. [ADR](phase-4-jobs-resource-status.md).

- **4A — One background operation.** Content-Aware Fill on the worker seam: progress, cancellation inside the Rust solve, stale-result refusal, one undo step.
- **4B — Resource accounting.** Unique retained buffers counted across document, undo, jobs and caches, under a working limit with eviction.
- **4C — Rendering and kernel contract.** Precision, profile and alpha semantics, region and preview-scale requests, cancellation and render failures ([contract](phase-4-rendering-contract.md); 13A succeeds it).
- **4D — Measured migration.** Spot healing and the slow save, RAW and transform paths on jobs; a save marks only its captured revision saved.

### 5. Native recovery and history

Goal: interrupted work is recoverable and a long session has bounded history. [ADR](phase-5-recovery-history-status.md).

- **5A — Recovery policy.** Autosave and recovery for dirty native (`.doctored`, or a legacy `.rz`), untitled and imported documents; an imported original is never overwritten.
- **5B — Recovery lifecycle.** Restart discovery, recover or discard, cleanup after a confirmed save, moved sources and failed writes.
- **5C — History panel.** History through the central undo path, selection history included, within a byte budget.
- **5D — Named snapshots.** Full editing states with persistence and memory accounting; layer comps are 12D-1.

### 6. Complete selection and mask workflows

Goal: create, refine and reposition a reusable mask. [ADR](phase-6-selection-mask-status.md); [selection kernels](phase-6-selection-quality.md), [cutout outputs](phase-6-cutout-output-contract.md).

- **6A — Selection essentials.** The Freehand Lasso, Color Range and selection-only transforms.
- **6B — Independent masks.** Mask link and unlink, mask-only transforms, persistent density and feather.
- **6C — Refinement workspace.** Select and Mask: inspection backgrounds, before/after, output to a mask or a new layer, and since 2026-10-11 Edge Detection, the Refine Edge brush and Decontaminate Colors. The matting quality gate stopped being a ship condition on 2026-10-10.
- **6D — Assisted selection.** Quick Selection, Select Objects and Select People, the Magnetic Lasso, Face Parts, Person Parts, Sky and the SAM 2.1 Object Finder. Magnetic Lasso Part B (⌥-drag freehand segments, the Caps Lock cursor, contrast keys, stylus pressure): [#17](https://github.com/kpgalligan/doctored/issues/17).

Vector masks shipped with the paths work ([ADR](v1-paths.md)).

### 7. High-depth document editing

Goal: higher precision survives editing and persistence. [Overview ADR](phase-7-high-depth-status.md); the [support matrix](phase-7-operation-support.md) says what works at depth, and the [carryover ledger](phase-7-carryovers.md), closed 2026-10-30, records each P7 row's outcome; its open rows are issues ([#2](https://github.com/kpgalligan/doctored/issues/2), [#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11), [#12](https://github.com/kpgalligan/doctored/issues/12), [#13](https://github.com/kpgalligan/doctored/issues/13)).

- **7A — Representation decision.** RGBA16 unsigned-normalized storage processed in f32, as a distinct typed image ([ADR](phase-7-high-depth-decision.md)).
- **7B — Neutral path.** 16-bit input kept through storage, groups, compositing, native reopen and PNG/TIFF output ([ADR](phase-7-neutral-path.md)).
- **7C — Tone and colour.** Adjustments and profile conversion on shared precision-independent kernels ([ADR](phase-7-tone-color.md)).
- **7D — Geometry, paint and retouch.** Masks, channels, transforms, paint, clone, healing and text/shape rasterization at 16 bits ([outcome](phase-7-geometry-paint-status.md), [pixel editing](phase-7-pixel-editing.md), [geometry](phase-7-geometry.md)).
- **7E — Public 16-bit mode.** Mode choice, deliberate conversion with export dithering, history and recovery ([ADR](phase-7-public-mode-status.md), [conversion and readout](phase-7-conversion-readout.md)).

### 8. Retained sources and revisitable RAW

Goal: RAW development and artwork can be revised after reopen at native precision. ADRs: [8A–8D](phase-8-retained-source-status.md), [8E](phase-8e-artwork-status.md), [8F](phase-8f-host-source-status.md), [8G](phase-8g-raw-enhancements-status.md).

- **8A — Embedded image source** and **8B — Linked asset workflow.** `REMOVED` — the feature was removed 2026-10-10; Place lands a raster layer.
- **8C — Retained RAW.** The camera file, recipe and decoder identity persist; RAW re-develops below retouch layers, never silently reinterpreted.
- **8D — Develop workflow.** 100% inspection, histogram and clipping, before/after, presets and sync, Display P3 output, explicit decoder migration.
- **8E — Retained artwork.** Text and shape descriptions re-render at native depth, with attributed colour runs and intrinsic colour glyphs.
- **8F — Host image and frame sources.** `REMOVED` — the feature was removed 2026-10-10; host stills and movie and Live Photo frames place as raster layers. Frames from SDR HEVC movies: [#15](https://github.com/kpgalligan/doctored/issues/15).
- **8G — Camera controls and local RAW.** Camera and lens controls, moiré, a profile-aware vignette and up to sixteen local masks in source coordinates with local tone, at most four of them changing white balance or camera exposure. Develop at 32-bit float (local tone map, Presence, Visualize HDR): [#4](https://github.com/kpgalligan/doctored/issues/4).

### 9. Editable photographic filters and retouching

Goal: retouching samples the intended content; selection and paint work at depth. ADRs: [9A–9D](phase-9-live-filters-status.md), [9D synthesis](phase-9d-synthesis-qualification.md), [9E](phase-9e-content-aware-status.md), [9F](phase-9f-selected-tone-status.md), [9G](phase-9g-selections-masks-status.md), [9H](phase-9h-paint-fill-status.md), [9I typed refinement](phase-9i-typed-cutouts-status.md), [9I gate v2](phase-9i-gate-v2-status.md).

- **9A–9C — Live filters.** `REMOVED` — the live-filter stack was removed 2026-10-10. Gaussian Blur, Unsharp Mask, High Pass, Reduce Noise and Dust & Scratches remain destructive, selection-aware filters.
- **9D — Retouch sampling.** Current, current-and-below and all-layers sampling; fractional and transformed clone and heal sources with overlays; destination expansion; native spot healing and red-eye.
- **9E — Content-aware workflow.** Source inclusion and exclusion brushes, an output destination, one prepared candidate for Apply, native Patch.
- **9F — Selected tone and scoped analysis.** Selection-limited tone and filters at depth, Auto analysis of the whole target or the weighted selection, fine parameter entry.
- **9G — High-depth selections, masks and planes.** Native scalar edits and displays, typed source selections, u16 selection and Quick Mask coverage, camera auxiliary mattes.
- **9H — High-depth paint and fill.** Pressure size and opacity, airbrush timing, paint modes, fills, gradients and dodge/burn at depth, with canonical dabs for replay.
- **9I — Cutout quality and typed refinement.** Refine Selection runs at u16 (the typed half of P7-12), and 6C's edge tools shipped at 8 and 16 bits. `DROPPED` (2026-10-30) — the assisted annotation run against gate v2: the gate is no longer a ship condition and nothing waits on it.

Frequency separation: [#17](https://github.com/kpgalligan/doctored/issues/17). Liquify with Reconstruct and Freeze Mask ships as a destructive filter. `DROPPED` (2026-10-30) — a backdrop filter layer: live filters were removed. `DROPPED` (2026-10-30) — a re-editable Liquify: re-editing is what a smart-object filter gives, and smart objects were removed.

### 10. Supported layered Photoshop exchange

Goal: exchange a defined subset of layered work through PSD with dependable appearance; Photoshop takes are the test reference.

- **10A — Import fidelity.** 8-bit RGB/gray PSD import with exact samples, ordinary masks, folder properties, ICC, PPI and a per-file loss report ([qualification](phase-10a-qualification.md); slices: [pixels](phase-10a-pixels-status.md), [masks](phase-10a-masks-status.md), [mask foundation](phase-10a-mask-foundation-status.md), [relative masks](phase-10a-relative-masks-status.md), [groups](phase-10a-group-import.md)).
- **10B — Basic layered export.** An 8-bit RGB PSD writer for rasters, masks, groups, clipping, opacity, blend modes, ICC and PPI behind a preflight, checked by reopening in GIMP ([ADR](phase-10b-export-status.md)).
- **10C — Depth and advanced constructs.** 16-bit PSDs both ways ([ADR](phase-10c-depth-status.md)); fourteen adjustment kinds (`psd_adjustment.rs`), layer styles (`lfx2`, `iOpa`, Blend If) and fill layers carried at both depths ([constructs](phase-10c-constructs-status.md)). Since then Grayscale, Lab, CMYK, 32-bit float (`Lr32`) and PSB files open and export, ZIP planes decode at 8, 16 and 32 bits (`psd_zip.rs`), and contours, noise, texture, patterns, bevel technique, glow range and glow gradients are carried (`psd_layer_effects.rs`). Measured kernel gaps: [#2](https://github.com/kpgalligan/doctored/issues/2); the Pattern Overlay angle: [#3](https://github.com/kpgalligan/doctored/issues/3).
- **10C-3 — PSD exchange follow-ups.** A Background layer kind (P7-38) and PSD layer comps (`cmls`): [#13](https://github.com/kpgalligan/doctored/issues/13). P7-39, `blend_interior_as_group` on a clip base, was verified 2026-10-13 (`core/tests/psd_knockout.rs`); a clip base below 100 % Fill is still unqualified: [#2](https://github.com/kpgalligan/doctored/issues/2).
- **10D — Native blending and styles.** Every blend mode, clipping, Blend If and layer styles at 16 bits, natively persisted ([basic](phase-10d-basic-blending.md), [separable](phase-10d-separable-status.md), [four](phase-10d-four-modes-status.md), [five](phase-10d-five-modes-status.md), [HSL](phase-10d-hsl-modes-status.md), [threshold](phase-10d-threshold-modes-status.md), [clipping](phase-10d-clipping-status.md), [Blend If](phase-10d-blend-if-status.md), [styles](phase-10d-layer-styles-status.md), [adjustments](phase-10d-adjustment-blending-status.md)); on CMYK adjustment layers too (`cmyk_style.rs`). The typed style pipeline and a float Layer Style preview: [#14](https://github.com/kpgalligan/doctored/issues/14).

Smart Object interchange: `REMOVED` — the feature was removed 2026-10-10; a PSD smart object imports as pixels.

### 11. Professional export and proofing

Goal: web and print deliverables without changing the master, and separately qualified linear-light and HDR workflows.

- **11A — Export workspace.** One render → resample → sharpen → convert → reduce → encode chain whose preview is the written bytes, with presets and naming tokens ([ADR](phase-11a-export-workspace-status.md)).
- **11B — Production outputs.** Per-layer and per-group exports, contact sheets and metadata policies through that chain ([ADR](phase-11b-production-outputs-status.md)).
- **11C — Output colour control.** LUT profiles (Convert to Profile into a LUT at 16 bits and the rest of the authored colours on a LUT-tagged document: [#16](https://github.com/kpgalligan/doctored/issues/16)), four rendering intents with black-point compensation, soft proofing, conversion policy ([ADR](phase-11c-output-color-status.md)). The soft proof's reference-CMS miss (mean ΔE76 1.87–5.59 against 2.0): [#16](https://github.com/kpgalligan/doctored/issues/16). `DROPPED` (2026-10-30) — the physical print-sample runbook: a manual session that gates nothing.
- **11D — Format additions.** HEIC export, 16-bit gray import, the metadata bridge, the float TIFF seam, PDF/SVG raster import, PDF export, AVIF and JPEG 2000 ([ADR](phase-11d-format-additions-status.md)); Gray export since shipped with [Grayscale mode](grayscale-mode.md). HEVC frames: [#15](https://github.com/kpgalligan/doctored/issues/15); PNG `gAMA`: [#16](https://github.com/kpgalligan/doctored/issues/16). `DROPPED` (2026-10-30) — the ProRes comparison (its frame sources were removed) and the float TIFF predictor (files are bit-exact without it).
- **11E — Linear-light evaluation.** Phase 1: a per-document `encoded` / `linear_light` policy for Normal and the 19 separable modes, opacity, masks, groups and pointwise adjustments ([ADR](phase-11e-linear-light-status.md)). Phases 2–3 (P7-24), Image Size and resampling in linear light, the remaining modes, styles, Blend If and a guided Shadows/Highlights: [#12](https://github.com/kpgalligan/doctored/issues/12).
- **11F — HDR and 32-bit float.** A linear-light float document mode with tone-mapped display and SDR output and exact float TIFF ([ADR](phase-11f-hdr-float-status.md)), since a full editing depth: paint, healing, styles, blend modes, F16 and OpenEXR export, the EDR canvas and conversion into float (`convert_precision` mode `rgba32`) ([v1 float editing](v1-float-editing.md), [HDR photos](v1x-hdr-photos.md)). The 11F ADR lists what float admits. Float plane edits: [#14](https://github.com/kpgalligan/doctored/issues/14). `DROPPED` (2026-10-30) — ACES-style tone maps: the clip and Reinhard maps and HDR Toning cover SDR delivery.

PDF/X and the CMYK open items: [#1](https://github.com/kpgalligan/doctored/issues/1), under which the document modes and spot channels shipped.

### 12. Dependable automation and productive workspaces

Goal: repeat edits reliably and manage a long session, with layer, geometry, clipboard and conversion parity at depth.

- **12A — Replay gaps.** A relative multi-layer Move step, a channel index fallback, canonical pressure and airbrush dabs, profile-qualified colours ([ADR](phase-12a-replay-status.md)).
- **12B — Reusable Actions.** Stable layer identity with `$layer` / `$channel`, relative and anchor geometry, named parameters, bundles, batch, and RAW preset repair ([ADR](phase-12b-actions-status.md)).
- **12C — Workspace productivity.** New Document with presets (8, 16 or 32 bits; RGB, Grayscale, CMYK or Lab), layer labels and search, the Navigator, custom shortcuts, saved workspaces ([ADR](phase-12c-workspace-status.md); keys: [shortcuts ADR](v1-shortcuts-fade.md)).
- **12D — Comparison views.** Layer comps; view rotation (View ▸ Rotate View…, the Hand tool's ⌥-drag, trackpad rotation) and synchronized second views ([ADR](phase-12d-comparison-views-status.md)). Match Zoom and Location across documents: [#20](https://github.com/kpgalligan/doctored/issues/20). `DROPPED` (2026-10-30) — a rotated Navigator thumbnail and persisted view rotation or count: view state with no undo or save value.
- **12E — High-depth layer and document operations.** Persisted layer ids, typed merges, locks, links and PPI edits, guides at depth, positional insertion, Stamp Visible and Flatten, knockout and `blend_interior_as_group` with PSD `knko` / `infx` ([ADR](phase-12e-layer-ops-status.md)).
- **12F — High-depth geometry.** Canvas Size, straighten, arbitrary rotation, the interactive transform, group and adjustment-content transforms, warp, at every depth ([ADR](phase-12f-geometry-status.md)).
- **12G — High-depth clipboard.** A typed internal payload for Cut, Copy, Copy Merged, Paste and New from Clipboard at 16 bits and float, explicit external conversion, editable text, shape and adjustment payloads ([ADR](phase-12g-clipboard-status.md)).
- **12H — Structure-preserving precision conversion.** `structure: preserve` in every direction beside the flattened conversion, with a plan of what is exact, changed, discarded or blocking, and one undo step ([ADR](phase-12h-conversion-status.md)).

### 13. Scale to large production documents

Goal: large high-depth multilayer documents meet declared latency, memory, save and recovery targets; until then Doctored makes no claim of dependable large multilayer high-depth editing.

- **13A — Regional evaluation.** All but 13A-latency implemented 2026-09-23 to 09-25: region and preview-scale rendering at 8, 16 and 32 bits against a full-frame reference, measured cancellation in preparation ([status](phase-13a-regional-status.md), [shipped runs](phase-13a-regional-shipped.md)). 13A-latency (a ≈2.2× regional slowdown) and P7-31: [#9](https://github.com/kpgalligan/doctored/issues/9).
- **13B — Tiled backing and history.** 30 runs (13B-1 to 13B-3) implemented 2026-09-25/26: byte and typed layer pixels in shared 256×256 tiles read and written in place ([status](phase-13b-tiled-status.md), [shipped runs](phase-13b-tiled-shipped.md)). 13B-4, -5, -6 and -9: [#10](https://github.com/kpgalligan/doctored/issues/10); 13B-7, the canvas on the regional path: [#9](https://github.com/kpgalligan/doctored/issues/9); 13B-8 is dropped.
- **13C — Incremental I/O and scratch.** Not started: bounded save and open with scratch, cancellable codecs, compact recovery and snapshot payloads (P7-32): [#11](https://github.com/kpgalligan/doctored/issues/11).
- **13D — Scale qualification.** Not started: a 24/48 MP corpus on the 16 GB minimum and a broader RAW camera corpus, published targets, stroke and text caps lifted by bounded processing (P7-33, P7-34): [#11](https://github.com/kpgalligan/doctored/issues/11).

**Phase 13 closes when** large multilayer high-depth editing stays within stated memory and interaction budgets; regional and tiled output agrees with the reference path, spatial edges and group effects included; recovery and native reopen hold with constrained scratch; and each promised size, format and history configuration finishes at its original dimensions, where missing samples, resource data, cancellation bounds or a refused stage fail it. The Phase 7 single-edit 24 MP result does not close 13D; regional rendering and tiled backing are separate deliverables, and a GPU path is an optimization, not completion.

## Open work

- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — 13A-latency and 13B-7: the regional slowdown and the canvas and previews on the regional path; closes P7-31.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — 13B-4, -5, -6, -9: tiled masks and channels, a measured history budget, one bounded cache registry, sampling plates.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 13C and 13D: bounded save and open, cancellable codecs, compact recovery, scale qualification, lifted caps (P7-32 to P7-34).
- `OPEN` [#12](https://github.com/kpgalligan/doctored/issues/12) — 11E phases 2–3 (P7-24): linear-light Image Size and resampling, the remaining modes, styles, Blend If and a guided Shadows/Highlights.
- `OPEN` [#13](https://github.com/kpgalligan/doctored/issues/13) — 10C-3: a Background layer kind (P7-38) and PSD layer comps; paths and vector masks in PSD and TIFF.
- `OPEN` [#14](https://github.com/kpgalligan/doctored/issues/14) — 10D and 11F: the typed layer-style pipeline, the float Layer Style preview, float plane edits.
- `OPEN` [#15](https://github.com/kpgalligan/doctored/issues/15) — 11D: SDR HEVC movie frames; multipage TIFF.
- `OPEN` [#16](https://github.com/kpgalligan/doctored/issues/16) — 11C's 16-bit LUT-profile conversion, the rest of the LUT-tagged authored colours and the soft-proof reference miss; 11D's PNG `gAMA` evaluation.
- `OPEN` [#17](https://github.com/kpgalligan/doctored/issues/17) — 6D's Magnetic Lasso Part B; frequency separation.
- `OPEN` [#18](https://github.com/kpgalligan/doctored/issues/18) — brush dynamics, the Pencil and erasers, brush, gradient and pattern libraries, Define Pattern.
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — font replacement and type on a path.
- `OPEN` [#20](https://github.com/kpgalligan/doctored/issues/20) — 12D's Match Zoom and Location, with 12B's and 12C's open items.
- `OPEN` [#2](https://github.com/kpgalligan/doctored/issues/2) — 10C's measured adjustment and effect-kernel gaps, and a clip base below 100 % Fill (P7-39).
- `OPEN` [#3](https://github.com/kpgalligan/doctored/issues/3) — the Pattern Overlay angle.
- `OPEN` [#4](https://github.com/kpgalligan/doctored/issues/4) — 8G and 11F: Develop at 32-bit float (local tone map, Presence, Visualize HDR).
- `OPEN` [#1](https://github.com/kpgalligan/doctored/issues/1) — PDF/X and the CMYK and spot-channel open items.
- `OPEN` [#5](https://github.com/kpgalligan/doctored/issues/5) — Lab Select Similar and Grow at 8 bits.
- `OPEN` [#6](https://github.com/kpgalligan/doctored/issues/6) — the TIFF spot open report.
- `OPEN` [#7](https://github.com/kpgalligan/doctored/issues/7), [#8](https://github.com/kpgalligan/doctored/issues/8) — HDR Toning's built-in presets and `.hdt` import.
- `OPEN` [#21](https://github.com/kpgalligan/doctored/issues/21) — local models for Select by text and Generative Fill and Expand.
- `OPEN` [#22](https://github.com/kpgalligan/doctored/issues/22) — 8G, 12D and 13A test gaps: the hanging 8C host probe, a rotated-view text commit, six load-sensitive test files.

## Follow-on tracks

Selectable work after the phases. An item with no issue is inventory in the feature catalogs ([gap review](photo-editor-gap-review.md), [September review](photo-editor-gap-review-2026-09.md), [GIMP study](gimp-image-manipulation-capabilities.md)).

- **Gradients and fills.** Fill layers and Pattern Overlay shipped. Gradient presets, Define Pattern, a pattern library, Pattern fill and multistop style gradients: [#18](https://github.com/kpgalligan/doctored/issues/18); the Pattern Overlay angle: [#3](https://github.com/kpgalligan/doctored/issues/3).
- **Brushes and swatches.** User brush presets, brush dynamics, the Pencil and the Background and Magic Erasers: [#18](https://github.com/kpgalligan/doctored/issues/18). Sampled tips and swatches are inventory.
- **Text and paths.** Attributed colour runs (8E), the Pen, the Paths panel and vector masks shipped ([ADR](v1-paths.md)). Font replacement, type on a path, path tools: [#19](https://github.com/kpgalligan/doctored/issues/19); paths in PSD and TIFF: [#13](https://github.com/kpgalligan/doctored/issues/13).
- **Styles and blending.** Blend If on adjustment layers, style presets, repeated effects, gradient glows and knockout shipped. Expandable effect rows in the Layers panel are inventory.
- **Geometry and deformation.** Trim, Reveal All, arbitrary rotation, perspective crop, crop aspect presets, warp and a destructive Liquify shipped; a re-editable Liquify is `DROPPED` (see Phase 9).
- **Photographic finishing.** The retouch brushes, Motion and Lens Blur, Tilt-Shift, targeted adjustments, Lens Correction ([ADR](v1-lensfun.md)) and Equalize shipped. Frequency separation: [#17](https://github.com/kpgalligan/doctored/issues/17). Match Color is inventory.
- **Multi-image photography.** Closed 2026-10-19 ([ADR](v1-stacks-status.md)): stacks, Auto-Align, Stack Mode, Merge to HDR, Auto-Blend, Merge Panorama, HDR Toning with user presets. Built-in presets: [#7](https://github.com/kpgalligan/doctored/issues/7); `.hdt` import: [#8](https://github.com/kpgalligan/doctored/issues/8).
- **Additional formats and sharing.** HEIC, AVIF, lossless JPEG XL and OpenEXR export, JPEG 2000 stills and GIF through Indexed Color shipped. Multipage TIFF and movie frames: [#15](https://github.com/kpgalligan/doctored/issues/15). Layer drag-out, a Share menu, multipage GIF and animation export are inventory.
- **Specialist production.** The CMYK, Lab, Grayscale, Indexed, Duotone and Bitmap modes and spot channels shipped; PDF/X and their open items: [#1](https://github.com/kpgalligan/doctored/issues/1); Lab Select Similar and Grow at 8 bits: [#5](https://github.com/kpgalligan/doctored/issues/5); the TIFF spot open report: [#6](https://github.com/kpgalligan/doctored/issues/6). Cloud Generative Fill and Expand shipped ([neural models](neural-models.md)); local models: [#21](https://github.com/kpgalligan/doctored/issues/21). Separations and artboards are inventory.
- **Distribution and adoption.** The signed, notarized release (`.github/workflows/publish.yml`) and the Sparkle updater it ships (Doctored ▸ Check for Updates…, [ADR](sparkle-updates.md); a proven self-update: [#24](https://github.com/kpgalligan/doctored/issues/24)) shipped, and the documentation site's source (`public-docs/`: a feature reference and the MCP tool reference) exists but is not yet published; practical user guides, rollback, and accessibility validation are inventory.
