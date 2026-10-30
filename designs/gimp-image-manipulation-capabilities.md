# GIMP architecture study — lessons and capability catalog (ADR)

- **Status:** Accepted — reference study (no work items of its own; gaps are owned by the [implementation plan](professional-photo-editor-implementation-plan.md))
- **Phase / dates:** pre-plan architecture study of the GIMP 3.3 development tree, 2026-08-10
- **Original:** [archive/gimp-image-manipulation-capabilities.md](../archive/gimp-image-manipulation-capabilities.md) — the full 14-section study with per-section implementation notes
- **Key commits:** `bc949f5` (study); it seeded [high-leverage-roadmap.md](high-leverage-roadmap.md) (`4f56283`) and [next-features.md](next-features.md) (`d0b0d26`)

## Context
GIMP delegates all pixels to GEGL (sparse, tiled, copy-on-write, disk-swappable graph buffers) and babl
(self-describing pixel formats), and fuses its document model with a live render graph. The study
paraphrased how GIMP implements compositing, projection, non-destructive filters, selections, colour,
filters, painting, transforms, text/paths, undo, performance and extensibility (65 layer modes, 49 tools,
~130 filters, 18 precision configurations, 803 scriptable procedures, 60+ formats), to decide which
mechanisms Doctored should copy and which were rewrite-scale for a Rust-core/Swift-shell editor.

## Decisions
Lessons adopted, and where Doctored applied them:
- **One grayscale coverage representation for selection, mask and channel** (§6) — canvas-sized coverage
  selections, layer masks and channels share it (u8 legacy, u16 native); Quick Mask and morphology fall out
  of it. Keep antialiasing (rasterization) and feathering (Gaussian post-process) distinct.
- **"Parameter object as source of truth, raster as cache"** (§11) for every re-editable layer — the opaque
  per-layer `meta` slot (a one-field parasite, §2) carries text, shapes and adjustment layers; later the
  Phase 8E artwork descriptions.
- **Composite in float regardless of storage** (§3) — the f32 compositor; later a per-document linear-light
  evaluation policy (11E) and float documents (11F).
- **One matrix pipeline, many front-ends; premultiplied resampling; exact fast paths for flips/90°** (§10) —
  `dr_doc_transform_layer`, perspective, the Phase 1D sampler rule.
- **Poisson healing on the difference field** (§9) — Healing/Spot Healing/Patch (`core/src/poisson.rs`).
- **Measured undo budget with a floor, named/nested groups** (§12) — Phase 5 byte budget with the
  nearest-step exception; history of immutable full states rather than swap-based command objects.
- **Serialize background jobs on one worker; do the work lazily/regionally before parallelizing or SIMD**
  (§13) — Phase 4 single worker; 13A regional render; 13B shared immutable 256×256 tiles (`core/src/tile_store.rs`).
- **Accumulate a displacement field for warps** (§10) — persisted warp fields (`core/src/doc_warp.rs`).
- **Distinguish assign from convert; per-document soft proof** (§7) — Assign/Convert Profile, Proof Setup.
- **One typed procedure registry as the scripting surface** (§14) — the MCP catalog (`AgentCatalog*.swift`)
  is the single agent/automation surface, with Actions and Batch replaying the same operations.

## Capability catalog (status verified 2026-09-26)
| Study area | GIMP capability | Doctored |
| --- | --- | --- |
| §2 Data model | Stable IDs/tattoos; locks; parasites; layer/channel/path trees | Stable layer identity (`doc_identity.rs`), Transparency/Pixels/Position locks (`doc_lock.rs`), `meta`, groups and channels. A flat per-document path list (`doc_path.rs`), no path tree |
| §2 Precision | 8/16/32-bit int, half, float × transfer curve | 8-bit, 16-bit and 32-bit float documents (Phases 7, 11F); no half/double |
| §3 Modes | 65 modes as a data table; blend/composite space/mode axes | 27 modes + Pass Through (`core/src/blend.rs`); document-level linear-light policy only — per-layer space axes not shipped |
| §4 Projection | Lazy dirty-tile cache, viewport-priority idle rendering | Regional render exists (13A) but the canvas display does not use it; no lazy tile projection |
| §5 Non-destructive | Live filter stacks with full compositing params, masks, split view | Adjustment layers. Live filter stacks shipped in 9A/9B and were removed for v1 on 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)) |
| §6 Selection | Coverage selections, Quick Mask, fuzzy select, Intelligent Scissors, Foreground Select | Coverage + Quick Mask, wand, Color Range, Quick Selection, Select Subject/Objects/People, Refine Selection with trimap/matting, Magnetic Lasso (live-wire edge tracing) |
| §7 Colour | babl fast path + lcms; LUT profiles; intents; soft proof; display filters | Matrix/TRC and LUT ICC (`core/src/icc*.rs`), intents, soft proof, assign/convert. CMYK document mode ([status](cmyk-mode-status.md)) and Grayscale document mode ([ADR](grayscale-mode.md)); Lab Color document mode, phases 1 and 1b (New, Mode both ways, view, `.rz`, layer structure and 15 blend modes, Fill, alpha channels, Lab TIFF, exports through RGB; [status](lab-mode.md)); Indexed Color document mode, closed (palette PNG/GIF/TIFF open; Mode ▸ Indexed Color with the Exact, Web, Uniform, System, Adaptive (median cut), Custom and Previous palettes and the None, Diffusion, Pattern and Noise dithers; Color Table and `.act`; painting, geometry and selections kept on the palette; the kept-palette PNG/GIF/TIFF export; `.rz`; [ADR](indexed-mode.md)); Duotone document mode, closed (a Gray document with 1–4 Lab inks, 13-slot curves and overprint colours: Mode ▸ Duotone… with Preview, the view through the inks, `.rz`, Duotone PSD/PSB, RGB/CMYK/Lab/Indexed Color and the exports through the inks, Grayscale and Bitmap through the inks' luminance, `.ado` files and the thumbnails through the inks; [ADR](duotone-mode.md)); Bitmap document mode, closed (one packed 1-bit plane: Mode ▸ Bitmap from 8-bit Grayscale and from Duotone through its Grayscale with 50% Threshold, Pattern Dither, Diffusion Dither, Halftone Screen in six spot shapes or Custom Pattern and the Output resolution, Grayscale back with a Size Ratio, `.rz`, 1-bit PNG, TIFF, BMP and PSD/PSB read and written, painting in black and white, the Gradient, Clone Stamp, History Brush, Liquify and the geometry thresholded onto the plane; every other tool refused for good; [ADR](bitmap-mode.md)); Multichannel document mode is not built, by decision ([ADR](multichannel-mode.md)); no display filters |
| §8 Filters | ~130 introspected operations; async histograms; HDR tone mapping | Hand-built set (blur, box/surface blur, sharpen, Unsharp, High Pass, noise, grain, dust, pixelate, emboss, edge detect…), schema-driven adjustment sheets, histograms/scoped statistics, Tone Map. No operation registry/introspection |
| §9 Paint | Dynamics, spacing in brush space, symmetry, smudge, ink, heal, MyPaint | Pressure size/opacity, airbrush timing, spline smoothing, 27 paint modes, clone/heal/dodge/burn, blur/sharpen/smudge/sponge and a history brush. No symmetry, ink or MyPaint |
| §10 Transforms | Unified/perspective/handle, warp, cage, N-point | Free Transform, perspective, warp fields, arbitrary canvas rotation, Liquify, Puppet Warp (as-rigid-as-possible pins on a triangle mesh) and Perspective Warp (joined planes, each a homography, with an ARAP extension). No cage or N-point |
| §11 Text/paths | Pango text with transforms; Bézier paths; vector layers | CoreText text layers with symbolic transforms; shape layers; Bézier paths with a Pen tool, a Paths panel and vector masks ([ADR](v1-paths.md)). No vector layers, text-to-path or text on path |
| §12 Undo | Swap-based command objects, tile-aligned snapshots, disk spill | Immutable full states + byte budget (Phase 5); tiled undo pending |
| §13 Performance | Tiled COW buffers, disk swap, mipmaps, runtime-dispatched SIMD | Arc-shared immutable planes, 13B tile store; no mipmaps, no disk swap, no hand SIMD |
| §14 Extensibility | PDB, out-of-process plug-ins, Script-Fu, 60+ formats | MCP server + built-in assistant + Actions/Batch; no plug-in API. Formats: PNG, JPEG, TIFF (8/16/float), BMP, GIF, WebP (lossless), HEIC (host), PSD 8/16, PDF export, RAW import |

## Rejected alternatives
- **Fusing the document model with a live render graph** (§1) — Doctored keeps pure immutable documents and
  renders them; a graph is rewrite-scale and couples the model to an engine API.
- **Adopting GEGL/babl** — the core is self-contained Rust; formats stay explicit typed interfaces.
- **Out-of-process plug-ins and generated bindings** — the MCP surface is the extension point.
- **Menus generated from an operation registry** — the catalog and sheets are hand-maintained (with
  `AdjustmentSchema.swift` for adjustment parameter UI).

## Consequences
- The study remains the mechanism reference when designing a feature: read the archived section before
  inventing an approach for compositing, projection, selection, paint or transforms.
- Its implementation notes (e.g. separate blend/composite spaces early, cache empty/full bits on masks,
  coalesce stroke invalidation) are advice, not commitments; the plan decides what ships.

## Open items (verified 2026-09-26)
The study records no to-do items of its own. Catalog gaps with a live owner:
- `TRACKED` — Per-layer blend/composite spaces and remaining linear-light migration → P7-24.
- `TRACKED` — Lazy/tiled projection for the canvas display and tiled undo → plan §13B (13A open item
  "Using the regional path for the canvas display").
- `DONE` — Bézier paths, the Pen, the Paths panel and vector masks ([ADR](v1-paths.md)).
- `TRACKED` — Text on path → plan track *Text and paths* (Liquify shipped);
  smudge and other per-dab brushes → *Photographic finishing*; CMYK/Lab modes → *Specialist production*;
  modern codecs (AVIF/JXL/EXR) and multipage TIFF → *Additional formats and sharing*.
- `DONE` — Magnetic lasso (Intelligent Scissors' live-wire trace) at 8 and 16 bits; ⌥-drag freehand segments, the
  Caps Lock cursor, contrast keys and stylus pressure are not built.
- `DROPPED` — Symmetry painting, ink/MyPaint engines, cage/N-point deformation, display filters, mipmaps,
  plug-in API: in no live plan track (the archived gap review rates illustration features P2/P3; the rest
  are rewrite-scale or replaced by the MCP surface).
