# Layer comps, view rotation and synchronized second views (ADR)

- **Status:** Accepted — open items remain ([#13](https://github.com/kpgalligan/doctored/issues/13), [#20](https://github.com/kpgalligan/doctored/issues/20), [#22](https://github.com/kpgalligan/doctored/issues/22))
- **Phase / dates:** 12D-1 (layer comps) and 12D-2 (view rotation + second view), both shipped 2026-09-19
- **Originals:** [archive/phase-12d-comparison-views-status.md](../archive/phase-12d-comparison-views-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-12d-comparison-views-status.md](../archive/2026-10/phase-12d-comparison-views-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `5adcb8d` (12D-1), `8c99934` (12D-2)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
The phase adds layer comps (named per-layer state) for comparing revisions, view rotation for painting at an
angle, and second views for watching detail and composition at once. Layer comps are a new persisted document
construct at both depths; rotation and second views are view state that touches no format and no document —
hence two runs.
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
  8 MiB `.doctored` payload cap, checked on the save/update op (so the refusal lands where the list grew, never on Save).
- **Purity:** every op returns `None` for "nothing would change" — no phantom undo step. Apply, update, rename,
  delete and cycling are each one undoable step.
- **Binding by layer id**; an entry whose id matches no layer is STALE, reported by its recorded name and skipped
  on apply. Rename does not stale; delete, merge and flatten do. A flatten drops the whole list. **Unrecorded
  layers:** apply hides every layer no entry names unless it is a group, at any depth, whatever the comp
  captured, lock-exempt — measured against Photoshop 26.10.0 (`../core/src/comps_unrecorded.rs`); derived at apply time,
  so no format change. A schema-1 layer renamed after save gets a HOLD row (id only, nothing recorded) on read,
  so it stays recorded and is never hidden.
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
  what the `.doctored` file carries (`comps_native::payload` already writes list order, so no format change) — is set by
  dragging a row in the window or by `reorder_layer_comp`. The core's `reorder` action takes `to` as the FINAL
  index (remove, then insert), like `reorder_layer`; `comp == to` is the exact no-op.
  It is bookkeeping, so no lock applies and no payload-cap check is needed. Drag only — no Move Up/Down buttons.
  `list_layer_comps` reports each entry's RECORDED state — `visible`, `offset`,
  `opacity`, `blend_mode`, `style` — in `get_document`'s vocabulary, each key present iff captured.
- **`update_layer_comp`** is the reconciliation point: `recapture: true` (default) re-records; `false` drops
  stale entries and refreshes bound names, keeping values. **Naming any capture flag states the whole set**
  (unnamed flags become `true`).
- **`.doctored` carriage** — `../core/src/comps_native.rs`, conditional outermost wrapper **43 (byte) / 44 (typed)**,
  one `LCMP` table: tag, `u32` schema, `u32` length, UTF-8 JSON (JSON because entries carry the style, already
  JSON everywhere). Written only when a comp exists, so a comp-less document's bytes equal the prior writer's.
  Schema 1 bound by index + name; **schema 2 (12E-1b) binds by the persisted layer id**
  ([phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md)) — survives reorder and rename. Stale entries
  write `"layer": null` so the writer never emits what its reader refuses. The binary word and JSON `schema`
  must agree; malformed tables refuse before any PNG decodes. Typed rebind runs after `materialize` mints ids.
- **Code:** `../core/src/doc_comps.rs`, `../core/src/precision_comps.rs`, `../core/src/ffi_comps.rs`
  (`dr_doc_layer_comps_json`, `dr_doc_edit_layer_comps`, `dr_precision_doc_layer_comps_json`; typed mutation via
  `dr_precision_doc_edit`), tests `../core/tests/layer_comps.rs`. Swift: `LayerComps.swift`
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
  reached by View ▸ Rotate View… (no chord: ⌥⌘R is Select ▸ Select and Mask…; live preview, Done or Cancel,
  Cancel restores), Hand-tool ⌥-drag (⇧ snaps 15°)
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
- **Shortcut matching is exact** (12D-2's extra-Shift leniency was a probe artifact): see
  [phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md), "The measured Shift rule".

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
  (`../core/src/comps_geometry.rs`); a literal `comps: self.comps.clone()` there leaves recorded comp positions unremapped.
  `../core/tests/layer_comps_geometry.rs` checks op→apply == apply→op over an exhaustive `GeometryEdit` match.

## Open items (verified 2026-10-07)
- `DONE` — apply hides unrecorded non-group layers
  (Binding above).
- `OPEN` [#13](https://github.com/kpgalligan/doctored/issues/13) — PSD `cmls` / image-resource-1065 comp
  import/export; a Photoshop-authored evidence fixture comes first.
- `DONE` — host-regenerated text and shape layers (fill layers too, `FillLayer.swift`): after a whole-canvas op the host sends one `{"op":"layer_comp","action":"shift"}` per re-rendered layer
  whose offset differs from the core's (`RasterDocument.followingHostPlacement`, `LayerComps.swift`; core
  `comps_shifted`, `../core/src/comps_geometry.rs`), inside the same edit. Measured by gate C9 of
  `scripts/verify_phase12d1_comps.py` (Image Size 260×195, Rotate 17°, Crop at 5°, 8 and 16 bits: entry = layer,
  apply `changed: false`); the 2026-09-28 drift was up to (+2, +2).
- `DROPPED` (2026-10-30) — a gate for real tracking-area and cursor-rect coverage under rotation: it needs
  real HID events and Accessibility permission, so it stays a manual check; hover and hit-testing are gated by
  the 9H case `rotate-view-hover-ui` (below).
- `OPEN` [#22](https://github.com/kpgalligan/doctored/issues/22) — typing and committing a text edit while
  the view is rotated: the 9H case `rotated-text-editor-ui` opens a text session at −30° and checks the mapping
  and hit test, but resets the rotation before its commit steps.
- `DONE` — hover and hit-testing under a rotated
  view are gated by the 9H case `rotate-view-hover-ui`; `setViewRotation` re-derives the brush ring's hover point
  after every turn (a turn need not scroll).
- `DONE` — recorded comp POSITIONS are remapped through
  every whole-canvas geometry op at both depths (Decisions, **Comps follow whole-canvas geometry**;
  `../core/tests/layer_comps_geometry.rs`).
- `DONE` — an entry on a layer with a retained source is
  remapped exactly, through the source bounds' `floor` (`sourced_offset`; `../core/tests/retained_geometry.rs`).
  The plain rule's `round` was 1 px off, enough to recreate an unremapped comp's false "locked" badge.
- `DONE` — comp reordering (Decisions, **Order is the
  user's**); gate C6 of `../scripts/verify_phase12d1_comps.py`.
- `DONE` — `list_layer_comps` and the save/update
  replies' `comp` row report each entry's recorded state (same decision); gate C7.
- `DONE` — 32-bit float arbitrary (filtered) canvas rotation is admitted with the rest of the
  filtered geometry; see [phase-12f-geometry-status.md](phase-12f-geometry-status.md).
- `DONE` — persisted layer ids and id-bound comps (`LCMP` schema 2, `../core/src/identity_native.rs`, `comps_native.rs`).
- `DONE` — typed locks, so typed `locked` is real (`../core/src/precision_lock.rs`, 12E-1b).
- `DONE` — destructive arbitrary rotation at 8 and 16 bits (12F-1b3 / 12F-1b4; P7-28 row in [the carryover ledger](phase-7-carryovers.md)).
- `DONE` — `set_view` malformed-argument refusals (12E-1, `viewArgument`); Shift-leniency conflict gap closed (12E-1b).
- `DROPPED` — group/adjustment position capture (decision above), a Rotate View rail tool, a rotated
  Navigator thumbnail, and persisting rotation / view count / sync in the document file, workspaces or
  Actions (all declared not scheduled or deliberate in the original).
- `OPEN` [#20](https://github.com/kpgalligan/doctored/issues/20) — Match Zoom and Match Location across open
  documents.
