# Feature survey (ADR)

- **Status:** Accepted — open items remain (the feature inventory was superseded by the gap review, `archive/photo-editor-gap-review.md`, and the [implementation plan](professional-photo-editor-implementation-plan.md)'s follow-on tracks)
- **Phase / dates:** pre-plan survey, 2026-08-11 → 2026-08-20
- **Original:** [archive/next-features.md](../archive/next-features.md) — full survey text and rationale
- **Key commits:** `d0b0d26` (survey), `3ac7cf1` (adjustment layers, clipping, eyedropper, morphology/Quick Mask), `7c25832` / `99554ec` (Select Subject, Subject tool), `e54f3c9` (Crop, Clone Stamp, Dodge/Burn, shapes, Zoom/Hand)

## Context
After the [high-leverage roadmap](high-leverage-roadmap.md) shipped, a feature survey ranked missing
features by value per unit of effort *given this architecture*: pure ops
over `Arc`-shared layers, canvas-sized coverage masks, an f32 compositor and the opaque per-layer `meta` slot.

## Decisions
- **`meta` is a parametric-layer mechanism, not a text mechanism.** Adjustment layers
  (`{type:"adjust", op, …}` contributing the adjustment applied to the accumulated backdrop), shape layers
  (`{type:"shape", kind, geometry, fill, stroke, radius}`, re-rendered like text) and layer styles all ride the
  existing slot with no format bump. *Why:* non-destructive editing without the live filter graph the GIMP
  study calls rewrite-scale; "adjustment + mask" is the most-used professional workflow.
- **Clipping masks are a compositing rule, not a model change**; layer groups are genuinely large (recursive
  private projections, pass-through later) and wait until stacks routinely exceed ~8 layers.
- **Cheap wins ride the coverage model:** Grow/Shrink/Border/Smooth are morphology like feather; Quick Mask is
  "~zero new machinery"; Save/Load Selection as channels; Curves is Levels with a spline LUT.
- **Retouch brushes extend the stroke-overlay path:** Clone Stamp (fixed-offset sampling, canvas-side overlay),
  Dodge/Burn as a core op (`core/src/doc_retouch.rs`); Healing is a Poisson solve on the source/destination
  difference (GIMP §9).
- **Subject selection via Vision drops straight into coverage.** Select Subject uses
  `VNGenerateForegroundInstanceMaskRequest` (`app/Sources/SubjectSelection.swift`; the reason the deployment
  floor went to macOS 15); MCP `select_subject` takes an `instance`. The Subject tool (W) hit-tests with ONE
  byte read of `VNInstanceMaskObservation.instanceMask` — a fixed 512×512 instance-ID map that is a straight
  squash of the image (not letterboxed; verified within one cell for aspect 0.31–3.2), so hover outlining is
  free and only a new subject pays for a mask.
- **Chosen sequence (shipped):** adjustment layers (Curves first) → clipping masks → eyedropper + crop →
  selection morphology + Quick Mask; then shapes, retouch brushes, groups, styles.
- **Keep deferring (then):** float/linear-light pipeline, live tiled render graph, colour management, smart
  objects, full pen/Bézier paths ("a subsystem, not a feature"; shapes deliver most of the value).

## Feature catalog (status verified 2026-09-26)
| Capability | Status |
| --- | --- |
| Adjustment layers (all ops incl. Curves), clipping masks | Shipped (`app/Sources/AdjustmentLayer.swift`, `core/src/adjust*.rs`) |
| Eyedropper, Crop tool with straighten, Zoom/Hand | Shipped (`app/Sources/Tools.swift`, `app/Sources/CropTool.swift`) |
| Shape layers rect/ellipse/line | Shipped (`app/Sources/ShapeLayer.swift`); polygon kind not shipped |
| Clone Stamp, Dodge/Burn, Healing, Spot Healing, Patch, Red Eye | Shipped (`core/src/doc_heal.rs`, `doc_retouch.rs`, `doc_redeye.rs`) |
| Sponge / Smudge / Blur / Sharpen brushes, History Brush | Shipped (`core/src/retouch_brush.rs`, `retouch_smudge.rs`, `doc_retouch_brush.rs`, `doc_smudge.rs`, 16-bit twins `precision_retouch_brush.rs`, `precision_smudge.rs`) |
| Layer groups (incl. pass-through), layer styles | Shipped (`core/src/doc_group.rs`, `core/src/style*.rs`) |
| Guides, rulers, grid, snapping | Shipped (`Guides.swift`, `RulerView.swift`, `ImageCanvasView+Grid.swift`, `SnapEngine.swift`) |
| Grow/Shrink/Border/Smooth, Quick Mask, Save/Load Selection, Channels panel | Shipped |
| Histogram, Curves, auto levels/contrast/colour | Shipped (`Histogram.swift`, `core/src/ops_auto.rs`) |
| Multistop gradients | Partial: Gradient Map editor (`GradientEditorView.swift`); paint tool and style panes not |
| History panel | Shipped (Phase 5, `DocumentHistory.swift`) |
| Copy Merged / Paste in Place | Shipped (`EditorViewController+Clipboard.swift`) |
| Paste Into (paste into selection as masked layer) | Shipped (Edit ▸ Paste Special ▸ Paste Into / Paste Outside, `PasteInto.swift`) |
| Trim / Reveal All (canvas) | Not shipped ("Reveal All" in the menu is the layer-mask command) |
| Select Subject, Subject tool, All People (merged `VNGeneratePersonSegmentationRequest`) | Shipped |
| Marching ants on the committed selection | Shipped (`ImageCanvasView+MarchingAnts.swift`) |
| Subject hover shimmer (won't do) | Not shipped |
| Per-person instances (`VNGeneratePersonInstanceMaskRequest`): Select ▸ Select People…, MCP `model "people"` | Shipped (`PersonInstances.swift`) |
| Content-aware fill (PatchMatch) | Shipped (9E, `core/src/doc_content_aware.rs`) |
| Colour management, float/linear-light | Shipped (Phases 3/7/11) |
| Smart objects | Not in v1. Smart objects, PSD smart-object read and write, and placed images kept as retained sources were built (Phase 8) and removed on 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)); a camera RAW layer keeps its original and re-develops; a PSD smart object imports as its pixels |
| Pen/Bézier paths, vector masks | Shipped ([ADR](v1-paths.md)) |
| Text on a path | Not shipped |

## Rejected alternatives
- Destructive-only shapes — parametric costs barely more and stays editable.
- A live filter graph for non-destructive editing — rewrite-scale; adjustment layers give most of the value.

## Consequences
- New re-editable layer kinds should use the `meta` description + raster cache pattern (later generalized by
  Phase 8E artwork descriptions) rather than new layer structs.

## Open items (verified 2026-09-26)
- `DONE` — Sponge/Smudge/Blur/Sharpen brushes and the History Brush (8 and 16 bits; options bar,
  MCP `blur_stroke` / `sharpen_stroke` / `smudge_stroke` / `sponge_stroke` / `history_brush_stroke`).
- `TRACKED` — Multistop gradients in the paint tool and style panes → track *Gradients and fills*.
- `TRACKED` — Trim / Reveal All → track *Geometry and deformation*.
- `DONE` — Pen/Bézier paths, the Paths panel, Make Selection / Fill Path / Stroke Path /
  Make Work Path and vector masks at 8, 16 and 32 bits, with MCP mirrors ([ADR](v1-paths.md)).
- `TRACKED` — Text on a path → track *Text and paths*.
- `TRACKED` — People-instance and specialized subject models → plan §6D ("individually selectable follow-ons").
- `DONE` — Edit ▸ Paste Special ▸ Paste Into (⌥⇧⌘V) and Paste Outside at 8/16/32 bits with MCP
  `paste {placement:"into"|"outside"}` (`app/Sources/PasteInto.swift`), on the byte `dr_doc_set_mask` primitive
  (`core/src/doc_mask_set.rs`) that also makes 8-bit From Selection canvas-sized.
- `DONE` — The committed selection's marquee marches, drawn by a `CAShapeLayer` overlay
  (`app/Sources/ImageCanvasView+MarchingAnts.swift`). The Subject-tool hover shimmer is not built (won't do);
  drag and lasso previews keep the static dash (`app/Sources/ImageCanvasView.swift` `strokeMarquee`).
- `DROPPED` — Polygon shape kind: shape layers support only rect/ellipse/line (`app/Sources/ShapeLayer.swift:237-246`).
- `DROPPED` — PSD smart-object interchange: "Full Smart Object interchange and PSB are separate commitments"
  (plan §10 boundary).
