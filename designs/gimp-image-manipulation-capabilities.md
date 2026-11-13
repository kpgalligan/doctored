# GIMP architecture study — lessons and capability catalog (ADR)

- **Status:** Accepted — reference study; open items remain ([#1](https://github.com/kpgalligan/doctored/issues/1), [#9](https://github.com/kpgalligan/doctored/issues/9), [#10](https://github.com/kpgalligan/doctored/issues/10), [#17](https://github.com/kpgalligan/doctored/issues/17), [#19](https://github.com/kpgalligan/doctored/issues/19), [#34](https://github.com/kpgalligan/doctored/issues/34)). The study has no work items of its own; those are the catalog gaps that have an issue.
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

## Capability catalog (status verified 2026-09-26; §7, §9 and §14 updated 2026-10-30)
| Study area | GIMP capability | Doctored |
| --- | --- | --- |
| §2 Data model | Stable IDs/tattoos; locks; parasites; layer/channel/path trees | Stable layer identity (`doc_identity.rs`), Transparency/Pixels/Position locks (`doc_lock.rs`), `meta`, groups and channels. A flat per-document path list (`doc_path.rs`), no path tree |
| §2 Precision | 8/16/32-bit int, half, float × transfer curve | 8-bit, 16-bit and 32-bit float documents (Phases 7, 11F); no half/double |
| §3 Modes | 65 modes as a data table; blend/composite space/mode axes | 27 modes + Pass Through (`core/src/blend.rs`); document-level linear-light policy only — per-layer space axes not shipped |
| §4 Projection | Lazy dirty-tile cache, viewport-priority idle rendering | Regional render exists (13A) but the canvas display does not use it; no lazy tile projection |
| §5 Non-destructive | Live filter stacks with full compositing params, masks, split view | Adjustment layers. Live filter stacks shipped in 9A/9B and were removed for v1 on 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)) |
| §6 Selection | Coverage selections, Quick Mask, fuzzy select, Intelligent Scissors, Foreground Select | Coverage + Quick Mask, wand, Color Range, Quick Selection, Select Subject/Objects/People, Refine Selection with trimap/matting, Magnetic Lasso (live-wire edge tracing) |
| §7 Colour | babl fast path + lcms; LUT profiles; intents; soft proof; display filters | Matrix/TRC and LUT ICC (`core/src/icc*.rs`), intents, soft proof, assign/convert. Document modes: CMYK ([status](cmyk-mode-status.md)), [Grayscale](grayscale-mode.md), [Lab Color](lab-mode.md), [Indexed Color](indexed-mode.md), [Duotone](duotone-mode.md) and [Bitmap](bitmap-mode.md), each mode's scope in its ADR; [Multichannel](multichannel-mode.md) is not built, by decision. No display filters |
| §8 Filters | ~130 introspected operations; async histograms; HDR tone mapping | Hand-built set (blur, box/surface blur, sharpen, Unsharp, High Pass, noise, grain, dust, pixelate, emboss, edge detect…), schema-driven adjustment sheets, histograms/scoped statistics, Tone Map. No operation registry/introspection |
| §9 Paint | Dynamics, spacing in brush space, symmetry, smudge, ink, heal, MyPaint | Pressure size/opacity, airbrush timing, spline smoothing, 27 paint modes, clone/heal/dodge/burn, blur/sharpen/smudge/sponge (`core/src/retouch_brush.rs`, `core/src/retouch_smudge.rs`) and a history brush. No symmetry, ink or MyPaint |
| §10 Transforms | Unified/perspective/handle, warp, cage, N-point | Free Transform, perspective, warp fields, arbitrary canvas rotation, Liquify, Puppet Warp (as-rigid-as-possible pins on a triangle mesh) and Perspective Warp (joined planes, each a homography, with an ARAP extension). No cage or N-point |
| §11 Text/paths | Pango text with transforms; Bézier paths; vector layers | CoreText text layers with symbolic transforms; shape layers; Bézier paths with a Pen tool, a Paths panel and vector masks ([ADR](v1-paths.md)). No vector layers, text-to-path or text on path |
| §12 Undo | Swap-based command objects, tile-aligned snapshots, disk spill | Immutable full states + byte budget (Phase 5); tiled undo pending |
| §13 Performance | Tiled COW buffers, disk swap, mipmaps, runtime-dispatched SIMD | Arc-shared immutable planes, 13B tile store; no mipmaps, no disk swap, no hand SIMD |
| §14 Extensibility | PDB, out-of-process plug-ins, Script-Fu, 60+ formats | MCP server + built-in assistant + Actions/Batch; no plug-in API. Formats: PNG, JPEG, TIFF (8/16/float), BMP, GIF, WebP (lossless), HEIC and AVIF (host), JPEG XL export (lossless), OpenEXR, Radiance HDR, PSD/PSB 8/16/32, PDF export, RAW import; a multipage RGB TIFF opens each page as a layer |

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

## Open items (verified 2026-10-30)
The study records no to-do items of its own. Catalog gaps and where they stand:
- `DONE` — The linear-light migration (§3, 11E, closed 2026-11-12). Per-layer blend/composite
  space axes stay catalog inventory (the §3 row), with no owner.
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — Lazy projection: the canvas display drawn
  through the regional render path (§4).
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — Tiled undo and the rest of the tiled
  backing (§12, §13).
- `DONE` — Bézier paths, the Pen, the Paths panel and vector masks ([ADR](v1-paths.md)).
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Text on a path (§11).
- `DONE` — Liquify, and Smudge and the other per-dab retouch brushes at 8 and 16 bits
  (`core/src/retouch_brush.rs`, `core/src/retouch_smudge.rs`).
- `DONE` — Lab Color, Grayscale, Indexed Color, Duotone and Bitmap document modes (their ADRs).
- `OPEN` [#1](https://github.com/kpgalligan/doctored/issues/1) — CMYK mode's remaining items and PDF/X
  ([status](cmyk-mode-status.md)).
- `DONE` — AVIF and lossless JPEG XL export, OpenEXR and Radiance HDR (§14).
- `DONE` — Opening multipage TIFF files: each page that matches page 1 opens as a layer at 8, 16 and 32 bits,
  in RGB, Grayscale, Lab Color and CMYK; a first page that opens as an Indexed or Bitmap document opens alone.
- `OPEN` [#34](https://github.com/kpgalligan/doctored/issues/34) — A chosen page of a multipage TIFF in Indexed
  Color and Bitmap, and Place of a multipage TIFF.
- `DONE` — Magnetic lasso (Intelligent Scissors' live-wire trace) at 8, 16 and 32 bits.
- `OPEN` [#17](https://github.com/kpgalligan/doctored/issues/17) — Magnetic Lasso part B: ⌥-drag freehand
  segments, the Caps Lock cursor, contrast keys and stylus pressure.
- `DROPPED` — Symmetry painting, ink/MyPaint engines, cage/N-point deformation, display filters, mipmaps,
  plug-in API (the archived gap review rates illustration features P2/P3; the rest are rewrite-scale or
  replaced by the MCP surface).
