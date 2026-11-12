# High-leverage roadmap — four features from the GIMP study (ADR)

- **Status:** Accepted — closed (historical record; not a feature catalog). All four phases shipped; the rewrite-scale deferrals were later taken up by the [implementation plan](professional-photo-editor-implementation-plan.md). The feature inventory is [photo-editor-gap-review.md](photo-editor-gap-review.md) with [photo-editor-gap-review-2026-09.md](photo-editor-gap-review-2026-09.md).
- **Phase / dates:** pre-plan feature roadmap, 2026-08-10 → 2026-08-11
- **Original:** [archive/high-leverage-roadmap.md](../archive/high-leverage-roadmap.md) — full history and per-phase design notes
- **Key commits:** `4f56283` (roadmap), `e7d23ca` (selection algebra), `e973b3c` (layer masks), `75ecde3` (text layers), `6c170ae` (free transform), `0c1aa52` (perspective)

## Context
Picked from the [GIMP architecture study](gimp-image-manipulation-capabilities.md) as the best
capability-per-effort fits for the model Doctored had then: canvas-sized u8 coverage selections, an f32
compositor and `Arc`-shared pure ops. The filter was "compounds with what exists, needs no rewrite-scale
commitment". Each phase shipped UI + MCP + (when state persists) `.rz`, core tests and README, through the
usual FFI three-file lockstep.

## Decisions
- **Order by leverage over cost: 1 → 2 → 3 → 4.** Selection algebra first (pure arithmetic on an already-right
  representation); masks and text adjacent so they share one format bump; transforms independent.
- **Phase 1 — selection algebra is per-byte arithmetic on coverage:** union `max(a,b)`, subtract
  `min(a,255−b)`, intersect `min(a,b)`, invert `255−a`; an all-zero result deselects; combining demotes exact
  path selections to mask kind. Modifiers decided at mouse-down (Shift add, Option subtract,
  Shift+Option intersect). Feather = Gaussian blur of the mask via in-place
  `dr_selection_feather(mask, w, h, radius)` (masks cross FFI as byte buffers, not handles). Mask selections
  draw marching ants from a cached marching-squares contour at coverage ≥ 128; geometric shapes keep exact
  paths. MCP: `mode: replace|add|subtract|intersect` on `select_*`, plus `modify_selection`.
- **Phase 2 — layer masks move with the layer** (GIMP-style, layer-sized, `Layer.mask` + `mask_enabled`,
  `core/src/doc.rs`); the compositor multiplies layer alpha by coverage (a three-input point op). Add
  (reveal/hide/from selection), delete, apply, enable/disable; painting a mask reuses the stroke overlay with
  `mask' = lerp(mask, luma(overlay), alpha(overlay))`. MCP `add_layer_mask`, `remove_layer_mask {apply}`,
  `set_layer_mask_enabled`, `target: "mask"`.
- **Phase 3 — parametric layers via an opaque `meta` slot:** `Layer.meta` is a JSON string the core stores,
  copies and serializes but never interprets (a one-field GIMP "parasite"). Text layers are raster layers
  whose meta holds the text parameters; CoreText renders in Swift (the core never shapes text); one atomic
  content+offset+meta call makes a re-edit one undo step (today `dr_doc_set_layer_content`). Destructive ops
  prompt "Rasterize text layer?" and drop meta; old readers see a plain raster. MCP `add_text_layer`,
  `edit_text_layer`. *Why:* "parameter object as truth, raster as cache" — reused later for shapes and
  adjustment layers ([next-features.md](next-features.md)).
- **Phase 4 — one matrix pipeline** (`dr_doc_transform_layer`, affine + sampler): inverse-mapped resample
  (nearest/bilinear/bicubic), extent from the transformed corner bbox, **premultiplied** f32 resample then
  unpremultiply (no dark fringes); exact 90°/flip ops stay fast paths. Free Transform (⌘T) with handles,
  Shift-constrain, rotate outside corners, Option-pivot, numeric angle/scale/size bound both ways to the
  matrix; cheap CG preview, one core resample on commit, one undo step. MCP `transform_layer` takes named
  params compiled to the matrix (friendlier for agents than raw matrices).
- **One `.rz` bump for masks + meta**, adding the meta field even before anything wrote it.
- **Deliberately deferred as rewrite-scale** (per the study): float/linear-light pipeline and blend/composite
  space axes, live non-destructive filter stacks, tiled/lazy buffers and mipmaps, colour management, layer
  groups and channels as objects.

## Rejected alternatives
- Rendering text in the core — shaping is an ecosystem (bidi, scripts, IME); delegate to CoreText.
- Raw matrices as the agent-facing transform API — named rotate/scale/translate is less error-prone.
- Two separate format bumps for masks and meta.

## Consequences
- The `meta` slot became the mechanism for every parametric layer type; its string cap (`MAX_DRDC_META_LEN`)
  is shared with layer styles.
- Every resampler must stay premultiplied (later enforced across the codebase by Phase 1D).

## Open items (verified 2026-10-30)
- `DONE` — Perspective transform (deferred within Phase 4): `0c1aa52`, `core/src/doc_perspective.rs`.
- `DONE` — Symbolic (re-rendering) transforms for text instead of rasterize-on-transform: 8E, P7-04 verified.
- `DONE` — PSD layer-mask import (stretch goal): 10A (`core/src/psd_raster_masks.rs`).
- `DONE` — Deferred rewrite-scale items since taken up by the plan: layer groups (`core/src/doc_group.rs`),
  channels (`core/src/doc_channel.rs`), colour management (ICC assign/convert, soft proof —
  `core/src/icc*.rs`, `core/src/soft_proof.rs`), high-depth and float documents (Phase 7, 11F). Live filter
  stacks (9A/9B) were also taken up, then removed for v1 on 2026-10-10
  ([removal ADR](convert-to-smart-object-status.md)).
- `DONE` — Linear-light compositing as a per-document evaluation policy (11E, every operator since
  2026-11-12); per-layer blend/composite-space axes are not built.
- `DONE` — The shared immutable tile store (13B-1, `core/src/tile_store.rs`). The rest of the tiled backing is
  [#10](https://github.com/kpgalligan/doctored/issues/10) and the canvas display through the regional render
  path is [#9](https://github.com/kpgalligan/doctored/issues/9); mipmaps are not planned.
- `DONE` — Move and Transform Selected Pixels rather than a whole layer: Move-tool floats, ⌘T and ⌥⌘T, the ⌥
  duplicates and Transform Again, on layers, masks, alpha channels and colour channels at 8, 16 and 32 bits
  (`core/src/selected_pixels*.rs`, `core/src/selected_plane.rs`, `app/Sources/MoveFloat.swift`).
