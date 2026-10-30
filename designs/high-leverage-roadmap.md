# High-leverage roadmap — four features from the GIMP study (ADR)

- **Status:** Accepted — closed (all four phases shipped; the rewrite-scale deferrals were later taken up by the [implementation plan](professional-photo-editor-implementation-plan.md))
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

## Open items (verified 2026-09-26)
- `DONE` — Perspective transform (deferred within Phase 4): `0c1aa52`, `core/src/doc_perspective.rs`.
- `DONE` — Symbolic (re-rendering) transforms for text instead of rasterize-on-transform: 8E, P7-04 verified.
- `DONE` — PSD layer-mask import (stretch goal): 10A (`core/src/psd_raster_masks.rs`).
- `DONE` — Deferred rewrite-scale items since taken up by the plan: layer groups (`core/src/doc_group.rs`),
  channels (`core/src/doc_channel.rs`), colour management (ICC assign/convert, soft proof —
  `core/src/icc*.rs`, `core/src/soft_proof.rs`), high-depth and float documents (Phase 7, 11F). Live filter
  stacks (9A/9B) were also taken up, then removed for v1 on 2026-10-10
  ([removal ADR](convert-to-smart-object-status.md)).
- `TRACKED` — Linear-light compositing is a per-document policy (11E); per-layer blend/composite-space axes and
  the remaining migration → P7-24 (row open).
- `TRACKED` — Tiled/lazy buffers → plan §13B (`core/src/tile_store.rs`, 13B-1); mipmaps are not planned.
- `DONE` — Transforming a floated selection (selected pixels) rather than a whole layer: with a
  selection active, ⌘T (8/16-bit) and `transform_selected_pixels` transform the active layer's
  selected pixels in one "Transform Selected Pixels" step; flagged open in
  `archive/photo-editor-gap-review-2026-09.md`. The first piece to land: the Move tool's drag/nudge
  and `move_selected_pixels` float the selected pixels by an integer delta through
  `SelectedPixelsEdit::MoveSelected` (`core/src/selected_pixels.rs`, 8/16-bit, the shared
  `move_extent` / `moved_pixel` body, `lock_block(Pixels)` directly) and the shared multi-layer
  sentence `MoveSelectedPixels.multiLayerRefusal(verb:count:)`. Then came
  `SelectedPixelsEdit::TransformSelected` (affine/quad + filter,
  `core/src/selected_pixels_transform.rs`) through the same two exports, the ⌘T floating session
  (`FloatingSelection.swift`, `EditorViewController+FloatingTransform.swift`), the `perspective`
  selection op, `transform_selected_pixels`, and `multiLayerRefusal(verb: "transform", …)`. Landed
  since: landing-sized plan scratch, one-wait commit, the opaque-bounds ⌘T box (the core float-rect
  query), selection-bounded plans and one 16-bit geometry wait, and the Move tool's ⌥-drag duplicate
  with a first-tick empty refusal (the persistent float shipped later: `MoveFloat.swift`;
  consecutive drags, nudges and `move_selected_pixels` calls lift ONCE from the float's source at
  its total delta, so pixels under the path come back; one "Move" step each, the float surviving
  undo and redo; Deselect, a selection or layer switch, Save and any other edit commit it; the ⌥
  copy floats and ⌥ over a float stamps a copy. Not measured: the scripted take's `move` event moved
  the whole layer, so it could not observe a float (`move_float_202_take.py`'s RESULT). Gray rides
  the same `move_selected` request and floats once lane A's Grayscale mode admits it), the rest of
  the ⌥ duplicate family — ⌥+arrow, ⌥-drag and ⌥+arrow with no selection duplicating the layers, ⌥⌘T
  Free Transform a Copy and the ⌥ cursor, each with its MCP `duplicate` — and the selected-pixels
  polish: the drag snapping to the float's opaque bounds, a delayed-reveal wait panel, the
  `selected_pixels_walk.rs` split and the named wrong-shape `bounds` refusal. Then ⌥⌘T with no
  selection duplicating the layers and transforming the copies, the delayed wait panel on every
  16-bit Move/⌘T/⌥⌘T/Crop commit and 8-bit layer transform and a duplicate drag or box snapping to
  its source, and Transform Again (⇧⌘T) and Transform Again with Copy (⌥⇧⌘T) with MCP
  `transform_again`, and the selected area of a targeted layer mask or alpha channel at 8, 16 and
  32-bit float — the same walk with a background-colour hole (`core/src/selected_plane.rs`),
  `target`/`background` on `transform_selected_pixels` and `transform_again`, and the Move tool and
  `move_selected_pixels` on the same planes, and a layer's selected pixels in a 32-bit float
  document, f32 through the same walk with filtered landings clamped to the floated pixels' range,
  and one colour channel's selected pixels at 8, 16 and 32 bits: the move fills the vacated area
  with the background swatch's 0.30/0.59/0.11 luma rounded to a code (unconverted at 32 bits) and
  refuses a lift from only transparent pixels, measured against Photoshop 26.10.0
  (`channel-move-633/take2`, `plane-gray-431-633/take1`).
