# Layer comps, view rotation and synchronized second views (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12D-1 (layer comps) and 12D-2 (view rotation + second view), both shipped 2026-09-19
- **Original:** [archive/phase-12d-comparison-views-status.md](../archive/phase-12d-comparison-views-status.md) — full history, the 12D-2 acceptance criteria, drive gates C1–C4 / V1–V6 / N1 and UI-probe cases, receipts
- **Key commits:** `5adcb8d` (12D-1), `8c99934` (12D-2)

## Context
12D adds layer comps (named per-layer state) for comparing revisions, a rotated view for painting at an angle, and
a second window for watching detail and composition at once. Layer comps are a new persisted document construct at
both depths; rotation and second views are view state that touches no format and no document — hence two runs.
Comps must stay distinct from Phase 5D's snapshots ([phase-5-recovery-history-status.md](phase-5-recovery-history-status.md)),
which restore the whole editing state.

## Decisions — layer comps (12D-1)
- **A comp is a named table of per-layer facts, never pixels**: `visible`; `offset` (RASTER entries only — a
  group's offset is a phantom, a typed adjustment has none); appearance = opacity + blend code + layer style as
  canonical JSON (Blend If travels inside the style at both depths). Three capture flags `visibility`,
  `position`, `appearance`; an off flag records nothing and apply leaves it alone. Every layer gets a row.
  Not recorded: selection, history, masks, channels, guides, canvas, profile, open state, locks, links,
  labels, clipping.
- **Limits:** 64 comps; names ≤ 128 bytes, trimmed, non-empty, unique (case-sensitive); comments ≤ 1024 bytes;
  8 MiB `.rz` payload cap, checked on the save/update op (so the refusal lands where the list grew, never on Save).
- **Purity:** every op returns `None` for "nothing would change" — no phantom undo step. Apply, update, rename,
  delete and cycling are each one undoable step.
- **Binding by layer id**; an entry whose id matches no layer is STALE, reported by its recorded name and skipped
  on apply. Rename does not stale; delete, merge and flatten do. A flatten drops the whole list.
- **Comps follow whole-canvas geometry:** Crop, Canvas Size, Image Size, the quarter turns, the flips,
  Rotate Canvas and Straighten remap each bound RASTER entry's recorded offset to the offset the op would have
  given that layer had it sat there — through the op's own per-layer expression, so applying the comp after the
  op lands every layer where applying it before would have (`../core/src/comps_geometry.rs`). An entry on a layer
  with an effective retained source is remapped through the source bounds. Image Size, and a Straighten or
  Rotate Canvas whose matrix scales, scale a recorded style by the factor they apply to the live one. Stale,
  group, position-less and (under Straighten) byte-adjustment entries are untouched; visibility, opacity and blend
  never change. The guarantee is PER OP: a chain of filtered ops can drift ±1 px, because after a filtered op a
  layer's size depends on its sub-pixel phase. Per-layer transforms (Free Transform, move, align, warp) leave
  comps alone.
- **Locks:** visibility and appearance are lock-exempt; position is not. A position-locked entry whose offset
  would change is reported `locked: true` (and `locked_count`), its offset skipped, the rest applied — a report,
  never a failure. Read with `lock_block(idx, EditKind::Position)` BEFORE the setter, because `under_locks`
  returns the same `None` for a lock refusal and a zero move. Real at both depths since 12E-1b (`precision_lock`).
- **Order is the user's:** the list order — what ◀ ▶ step through, what `list_layer_comps` returns and
  what the `.rz` carries (`comps_native::payload` already writes list order, so no format change) — is set by
  dragging a row in the window or by `reorder_layer_comp`. The core's `reorder` action takes `to` as the FINAL
  index (remove, then insert), like `reorder_layer` / `reorder_live_filters`; `comp == to` is the exact no-op.
  It is bookkeeping, so no lock applies and no payload-cap check is needed. Drag only — no Move Up/Down buttons.
  `list_layer_comps` reports each entry's RECORDED state — `visible`, `offset`,
  `opacity`, `blend_mode`, `style` — in `get_document`'s vocabulary, each key present iff captured.
- **`update_layer_comp`** is the reconciliation point: `recapture: true` (default) re-records; `false` drops
  stale entries and refreshes bound names, keeping values. **Naming any capture flag states the whole set**
  (unnamed flags become `true`).
- **`.rz` carriage** — `../core/src/comps_native.rs`, conditional outermost wrapper **43 (byte) / 44 (typed)**,
  one `LCMP` table: tag, `u32` schema, `u32` length, UTF-8 JSON (JSON because entries carry the style, already
  JSON everywhere). Written only when a comp exists, so a comp-less document's bytes equal the prior writer's.
  Schema 1 bound by index + name; **schema 2 (12E-1b) binds by the persisted layer id**
  ([phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md)) — survives reorder and rename. Stale entries
  write `"layer": null` so the writer never emits what its reader refuses. The binary word and JSON `schema`
  must agree; malformed tables refuse before any PNG decodes. Typed rebind runs after `materialize` mints ids.
- **Code:** `../core/src/doc_comps.rs`, `../core/src/precision_comps.rs`, `../core/src/ffi_comps.rs`
  (`rz_doc_layer_comps_json`, `rz_doc_edit_layer_comps`, `rz_precision_doc_layer_comps_json`; typed mutation via
  `rz_precision_doc_edit`), tests `../core/tests/layer_comps.rs`. Swift: `LayerComps.swift`
  (`RasterDocument.editingLayerComps(_:)` is the ONE writer), `ImageDocument+LayerComps.swift`,
  `LayerCompsWindowController.swift`, `LayerCompSheets.swift`, `AgentServer+LayerComps.swift`.
- **UI is a window** (Window ▸ Layer Comps…, Snapshots precedent), not a sixth panel tab. **Host addresses comps by
  NAME** (trimmed, `compNamed`), core by index resolved immediately before the edit — an index means nothing on
  another document. `lastAppliedLayerComp` (for ◀ ▶) is view bookkeeping: not persisted, not undone.
- **MCP:** `list_layer_comps` (read), `save_layer_comp`, `apply_layer_comp`, `update_layer_comp`,
  `delete_layer_comp`, `reorder_layer_comp` (recordable); all admitted at 8, 16 and float.

## Decisions — view rotation and second view (12D-2)
- **Rotation turns the VIEW only**: `ImageCanvasView` sits in an unrotated `CanvasHostView` (the scroll view's
  document view) and only its `frameCenterRotation` changes; the canvas keeps document-sized bounds, so every
  pointer site's `convert(event.locationInWindow, from: nil)` still lands in document space and no call site
  changed. Positive = CLOCKWISE on screen; normalized into (−180, 180]. At 0° `ViewRotation` is the identity and
  the rotation is assigned only when it differs, so 0° numbers are bit-identical to 12C-2's. Writes nothing:
  no pixel, history, dirty flag, Action step or saved state.
- **One writer**, `EditorViewController.setViewRotation` (keeps the canvas point under the well's centre fixed),
  reached by View ▸ Rotate View… (⌥⌘R; live preview, no Apply, Cancel restores), Hand-tool ⌥-drag (⇧ snaps 15°)
  and trackpad rotate. Reset View Rotation is ⌃⌥⌘R.
- **Rulers hide while turned** (`rulersVisible = pref && viewRotation == 0`; preference untouched). Grid,
  guides, marquee, handles rotate for free (drawn in canvas bounds). The Navigator proxy and `get_view.visible`
  are the axis-aligned bounding box of the turned well. Interpolation goes `.high` while turned.
- **Rotation is owned by the WINDOW**; Sync Pan & Zoom does not carry it.
- **A second view is a second `EditorWindowController` on the same `NSDocument`** — one picture, history and
  undo stack by construction. Per view: zoom, scroll, rotation, Navigator/Info, panel tab, canvas sessions,
  frame and `<name>:N` title. Per document: layer selection, paint target, channel eyes, selection, guides,
  ruler origin, comps, Sync Pan & Zoom.
- **Cross-view mirroring uses its own notification**, `.imageDocumentViewTargetsDidChange`
  (`ImageDocument+Views.swift`), posted from `oldValue !=`-guarded `didSet`s only when >1 window, not
  publishing, not navigating history — so a one-window document behaves exactly as before. Reusing
  `.imageDocumentImageDidChange` was rejected: it fires every drag tick and inside publication windows.
- **`releaseDisplayForWindowClose()` removes the editor's document observers** — a closed window no longer
  means a closed document. It also removes the
  Layers, Channels and Info panels' observers (`releaseForWindowClose()`), so a closed view's panels stop
  reloading on every edit.
- **MCP:** `set_view` gains `rotation`, `fit`, `sync_pan_zoom`, `view`; `open_second_view`,
  `close_second_view`, `list_views`; all `nonRecordable`. **An omitted `view` is ALWAYS view 1** (never the key
  window, which is nil while inactive). Ordinals are minted once and never reused. `sync_pan_zoom` is
  document-wide view bookkeeping (not persisted/undone). The last view cannot be closed. Malformed view
  arguments are refused by name (`viewArgument`, `AgentServer+Views.swift`, since 12E-1).
- **New View and Sync Pan & Zoom take no chord.**
- **Shortcut matching is exact.** The 12D-2 claim that `NSMenu.performKeyEquivalent` ignores an extra Shift was
  an artifact of probe-built `NSEvent.keyEvent` presses; 12E-1b measured hardware-shaped events match chords
  EXACTLY, so `ShortcutMenu.conflict`'s exact comparison is correct and `X`/`⇧X` pairs work.

## Rejected alternatives
- Rotating the scroll view's document view — would turn scrolling, hand pan and clip constraints with it.
- Merging a named capture flag into the stored set — the same call would mean different things on different comps.
- A panel tab for comps — the right panel's tabs are per-document inspectors; a sixth would not earn its width.
- Hanging view mirroring on `.imageDocumentImageDidChange` — would change single-window undo/redo/revert delivery.

## Consequences
- Canvas code must keep reading pointer locations via `convert(_:from:)` and drawing in canvas bounds, or it
  breaks under rotation.
- Per-document state that views should share must live on `ImageDocument`/`RasterDocument` and post
  `.imageDocumentViewTargetsDidChange` under the same guards; per-view state stays on the editor.
- Any new layer-removing or layer-merging op must decide what it does to comps (ids stale by design).
- **Invariant:** every new whole-canvas geometry op builds its comps through `remapped_comps`
  (`../core/src/comps_geometry.rs`); a literal `comps: self.comps.clone()` there brings the false "locked" badge back.
  `../core/tests/layer_comps_geometry.rs` checks op→apply == apply→op over an exhaustive `GeometryEdit` match.

## Open items (verified 2026-09-26)
- `TRACKED` — Photoshop `cmls` / image-resource-1065 comp import/export; evidence fixture must be authored first → plan §10C-3.
- `DONE` — recorded comp POSITIONS are remapped through every whole-canvas geometry op at both depths
  (`../core/src/comps_geometry.rs`, decision above; `../core/tests/layer_comps_geometry.rs`).
- `DONE` — retained-source entries: MEASURED at 1 px (8×8 → 11×9 Image Size takes an entry at
  (2, 2) to (2, 2) through the source bounds' `floor`, where the plain rule's `round` gives (3, 2)), material
  because 1 px recreates the false "locked" badge, so sourced entries are now remapped exactly
  (`sourced_offset`; `../core/tests/retained_geometry.rs`).
- `OPEN` — host-regenerated TEXT and SHAPE layers: the host re-renders a described layer after
  the op at an offset of its own, so its remapped entry can sit off the layer. MEASURED over MCP (2026-09-28, a
  200×150 document, a 24 px text layer and a 40×30 rect, identical at 8 and 16 bits; entry − layer): Image Size
  200×150 → 260×195 text (+2, +2), shape (+1, +1); Rotate 17° (+1, +1) both; Crop with a 5° straighten (+1, +1)
  both; Crop, Canvas Size, Rotate 90° and Flip 0. Rasters: 0 on every op. Not 0, so DISCLOSED and open: after
  such an op a position-locked text or shape layer is reported `locked` by its comp (the false badge), and an
  unlocked one jumps by the drift when the comp is applied. The fix is a per-entry-offset
  `layer_comp` request the host sends after it re-renders (follow-up).
- `DONE` — comp reordering: the core `reorder` action at both depths, drag-to-reorder in the
  Layer Comps window and the recordable `reorder_layer_comp` tool; gate C6 of `../scripts/verify_phase12d1_comps.py`.
- `DONE` — `list_layer_comps` (and the save/update replies' `comp` row) reports each
  entry's recorded `visible`, `offset`, `opacity`, `blend_mode` and `style`; gate C7.
- `DONE` — hover and hit-testing under a rotated view are gated by the 9H case
  `rotate-view-hover-ui`, and `setViewRotation` re-derives the brush ring's hover point after every turn (a turn
  need not scroll, so the stale ring it left is fixed).
- `OPEN` — real tracking-area and cursor-rect coverage under rotation is hand-checked by
  necessity: synthetic moves never reach tracking areas, and real HID events need Accessibility permission. Hand
  checks: (a) the ring follows the pointer over the picture at 30°; (b) over the host corners the pointer is the
  tool cursor, with no ring pinned to the edge; (c) at 30°, typing −30 in Rotate View… moves the ring with the
  resting pointer.
- `OPEN` — text-session editing under a rotated view is hand-checked only, no gate; deferred.
- `DONE` — 32-bit float arbitrary (filtered) canvas rotation is admitted with the rest of the
  filtered geometry; see [phase-12f-geometry-status.md](phase-12f-geometry-status.md).
- `DONE` — persisted layer ids and id-bound comps (`LCMP` schema 2, `../core/src/identity_native.rs`, `comps_native.rs`).
- `DONE` — typed locks, so typed `locked` is real (`../core/src/precision_lock.rs`, 12E-1b).
- `DONE` — destructive arbitrary rotation at 8 and 16 bits (12F-1b3 / 12F-1b4; P7-28 row in [phase-7-carryovers.md](phase-7-carryovers.md)).
- `DONE` — `set_view` malformed-argument refusals (12E-1, `viewArgument`); Shift-leniency conflict gap closed (12E-1b).
- `DROPPED` — group/adjustment position capture (decision above), a Rotate View rail tool, Match Zoom/Location
  across documents, a rotated Navigator thumbnail, and persisting rotation / view count / sync in `.rz`,
  workspaces or Actions (all declared not scheduled or deliberate in the original).
