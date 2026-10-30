# Phase 12D — Revision/comparison views: status

Slice 12D of [the professional photo editor plan](professional-photo-editor-implementation-plan.md)
is delivered in **two bounded runs**, as the request's item 4 allows:

- **12D-1 — LAYER COMPS. Shipped 2026-09-19.**
- **12D-2 — view-only rotation + a synchronized second view. Shipped
  2026-09-19**, to the acceptance criteria 12D-1 fixed; its own section below
  records what shipped against each of them.

The split is the honest one: layer comps are a new persisted document
construct at both depths with an `.rz` version bump, five MCP tools, a window
and a sheet; view rotation and second views are view-state work that touches
no format and no document. Delivering both in one run would have meant one of
them arriving untested.

## 12D-1 scope

A **layer comp** is a named recording of per-layer state — never pixels. It
is deliberately the opposite of Phase 5D's named snapshots: a snapshot holds
the whole editing state (pixels, selection, edit target) and restoring it
puts every byte back; a comp holds a table of small
per-layer facts and applying it is an ordinary undoable property edit. The
sentence in [the Phase 5 status](phase-5-recovery-history-status.md) that
distinguished the two now points here.

### The comp contract

| Question | Answer |
| --- | --- |
| What is recorded | Per layer: `visible`; `offset` (RASTER entries only); appearance = opacity + blend-mode code + the layer style as canonical JSON |
| Which attributes | The comp's own three flags — `visibility`, `position`, `appearance` — fixed when it is saved and changeable with `update_layer_comp`. A flag that is off records nothing and applying the comp leaves that attribute alone |
| Which layers | EVERY layer. A comp is a whole-document table; a layer with no row would silently keep whatever state the document was in, which is the one thing a comp exists to remove |
| How a layer is named | The core's stable session identity (`doc_identity.rs`), with the name kept beside it |
| What is NOT recorded | Pixels, the selection, the history, the Quick Mask, masks, channels, guides, the canvas, the profile, group open/closed state, locks, links, colour labels, clipping |
| Limits | 64 comps per document; names ≤ 128 bytes, trimmed, non-empty and unique (case-SENSITIVE); comments ≤ 1024 bytes; an 8 MiB `.rz` payload cap |
| Purity | Every op answers "nothing would change" with `None`, so no phantom undo step is ever registered |

**Blend If is not in the appearance set as a separate field, and does not need
to be**: at both depths it travels inside the layer style JSON
(`style_model::BlendIf` is a field of `LayerStyle`, and the typed
`reported_style()` merges the layer's own `blend_if` back into it), so a comp
that captures appearance captures Blend If with it.

### The binding rule and staleness

An entry binds by layer id. An entry whose id no longer matches any layer is
**stale**: it is reported under the name the layer carried when the comp was
recorded, `apply` skips it, and the report and every MCP reply carry the name
rather than silently dropping the row. A RENAME does NOT make an entry stale —
the id carries through it. A delete, a merge and a flatten do.

**Across a SAVE, 12D-1 bound by INDEX + NAME; since
[12E-1b](phase-12e-layer-ops-status.md) it binds by the PERSISTED id**
(`LCMP` schema 2, written by both depths). The in-session rule above is
unchanged; what changed is that reopening a file no longer needs the layer to
be at the same index under the same name. A reopened comp now survives a
reorder AND a rename, and an entry whose id no record carries is stale, which
is what it always meant.

`update_layer_comp` is the reconciliation point: with `recapture: true` (the
default) the whole table is re-recorded from the document; with
`recapture: false` the comp's stale entries are DROPPED and its bound entries
take their layers' current names, while the recorded values stay.

### The lock rule

Visibility and appearance are lock-EXEMPT, on the colour-label and rename
precedent: they change no pixel and no coordinate. **Position is not** — it is
exactly what `LOCK_POSITION` forbids. A bound entry whose recorded offset
differs from the layer's current one on a position-locked layer is reported
`locked: true` (per entry, and counted in `locked_count`), its offset is
skipped, and its visibility and appearance still apply. A locked entry is a
REPORT, never a failure.

The lock is read with `lock_block(idx, EditKind::Position)` BEFORE the setter
runs, because `under_locks` answers a lock refusal with the same `None` a
zero-length move answers (`doc_lock.rs:242–252`), so the setter itself can
never tell the host which of the two happened.

A typed layer carried no locks in 12D-1, so `locked` was always `false` at
high depth. **Since [12E-1b](phase-12e-layer-ops-status.md) a typed layer
carries the same four bits** (`precision_lock`), so `comp_entry_locked` is a
real question at both depths and `locked` is a real answer — computed the same
way, with `lock_block(idx, EditKind::Position)` read BEFORE the apply.

### `.rz` carriage and the persisted-identity decision

`comps_native.rs` is the conditional outermost wrapper, on `label_native.rs`'s
shape: **byte 43 / typed 44** before every prior version word (41/42
included), and one `LCMP` table after every prior tail (`LLBL` included) —
tag, `u32` schema = 1, `u32` length, then that many bytes of UTF-8 JSON.
Written ONLY when the document holds a comp, so **a comp-less document's bytes
are exactly the 41/42 writer's, first word included**, and deleting the last
comp restores them. `RZDC_VERSION` is 44.

The payload is JSON rather than a packed table because an entry carries a
layer STYLE, which is already JSON everywhere else in the crate; a second
binary encoding of that schema would be a second implementation of it.

**The persisted-identity decision.** Layer ids are deliberately session-scoped
and NOT persisted (`doc_identity.rs`; 12E owns the persisted-id wrapper, and
that contract is unchanged by this slice). A comp therefore persists by
**index + name**:

- A BOUND entry writes the index the layer sits at right now and the name it
  carries right now, so an in-session rename survives the reopen.
- A STALE entry writes `"layer": null` and its last-known name, and the reader
  accepts that as stale. The writer must never emit a file its own reader
  refuses, which is why the `>= records` and duplicate-index refusals apply to
  BOUND entries only.
- The reader re-binds an entry only when the layer at that index still carries
  that name; anything else reopens stale, which is what it would be.

Byte indices are bottom-first (`MOUT`'s convention); typed indices are the
flattened postorder record index (`LLBL`'s / `LCLP`'s). At the typed depth
`parse` runs at the usual place — before any pixel decodes — and `rebind` runs
after construction on BOTH of the reader's exits, because the pending nodes
have no ids until `materialize` mints them.

Refused before a single PNG decodes: a schema other than 1 (in the header word
AND in the JSON), a zero or over-8 MiB length, non-UTF-8 bytes, malformed
JSON, an unknown key, more than 64 comps, an empty list, an empty/oversize
name or comment, an empty capture set, a duplicate comp name, a bound index
past the record count or repeated within one comp, a half-specified
appearance, a non-finite opacity, and trailing bytes.

### Where the code lives

| Side | Files |
| --- | --- |
| Model + ops | `core/src/doc_comps.rs` (byte), `core/src/precision_comps.rs` (typed twin + `CompEdit`) |
| Format | `core/src/comps_native.rs`, wired in `rzdc.rs` / `rzdc_read.rs` / `precision_native.rs` / `precision_native_read.rs` / `precision_native_plan.rs` |
| FFI | `core/src/ffi_comps.rs` (`rz_doc_layer_comps_json`, `rz_doc_edit_layer_comps`, `rz_precision_doc_layer_comps_json`), the typed mutation through `rz_precision_doc_edit` |
| Tests | `core/tests/layer_comps.rs` (15 tests) plus one `#[cfg(test)]` unit test in `doc_comps.rs` for the payload cap |
| Swift model | `app/Sources/LayerComps.swift` — `RasterDocument.editingLayerComps(_:)` is the ONE writer the window, the menu and MCP share |
| Swift UI | `ImageDocument+LayerComps.swift` (five UI-only entry points), `LayerCompsWindowController.swift`, `LayerCompSheets.swift` |
| MCP | `AgentServer+LayerComps.swift` (five handlers + catalog) |
| Drive | `scripts/verify_phase12d1_comps.py` |

### UI and MCP

**Window ▸ Layer Comps…** — a window on `SnapshotsWindowController`'s shape
(the panel's five tabs are already the per-document inspectors, and the list is
consulted beside the canvas while comps are cycled). One row per comp with the
`V P A` capture glyphs and `stale N` / `locked N` badges; New…, Apply, ◀ ▶
(apply the previous/next comp by list order), Update (re-record in place),
Options… (name, comment, flags, optional re-record) and Delete. Refusals are
the core's own sentence in the status line, beeped.

**MCP**, all five with `document_id` and all addressing a comp by NAME:

| Tool | Mirrors | Recordable |
| --- | --- | --- |
| `list_layer_comps` | the window's list | no (a read) |
| `save_layer_comp {name, comment?, visibility?, position?, appearance?}` | New… | yes |
| `apply_layer_comp {name}` → `{ok, changed, applied, stale, locked, document}` | Apply / ◀ ▶ | yes |
| `update_layer_comp {name, new_name?, comment?, flags?, recapture?}` | Update / Options… | yes |
| `delete_layer_comp {name}` | Delete | yes |

All five are admitted at both depths (`DocumentPrecision.commonTools`) and on a
32-bit float document (`FloatDocument.tools`), because none of them touches
pixel storage.

## Decisions taken here

1. **A window, not a panel tab.** The Snapshots precedent; the right panel's
   five tabs are the per-document inspectors and a sixth would not earn its
   width.
2. **Comps are addressed by NAME on the host side and by INDEX in the core
   request.** An index means nothing on another document, which is what a
   recorded Action replays on; the core refuses a duplicate name, so the name
   is a unique handle. The handlers resolve name → index immediately before
   the edit.
3. **Naming ANY capture flag states the WHOLE set**; an unnamed flag becomes
   `true`, the same default a save with no flags takes. The alternative —
   merging one named flag into the comp's stored set — makes the same call
   mean two things on two comps. Stated in the header, the catalog and the
   tool descriptions.
4. **Position is recorded for RASTER entries only.** A group's offset is a
   phantom derived from its descendants and a typed adjustment has none, so
   recording one would record a number the writer never meant.
5. **The payload cap is checked on the COMP op.** `save` and `update` build
   the `.rz` payload for the resulting list and refuse with "layer comps
   exceed the `.rz` payload cap", so the refusal lands where the list grew and
   never on the Save that would otherwise be the first call unable to write
   the file.
6. **A flatten drops the whole comp list**; every entry's identity is gone.
   Every geometry op (rotate, flip, crop, canvas resize, image resize) carries
   it, and recorded positions apply VERBATIM through them — remapping them
   through canvas geometry is recorded below as a limit, not done.
7. **A comp records no pointer back to itself**, so the window's ◀ ▶ step from
   `ImageDocument.lastAppliedLayerComp`, which is VIEW bookkeeping: not part of
   the editing state, never persisted, never undone.

## Open, with owners

| Gap | Owner |
| --- | --- |
| Photoshop's own `cmls` layer comps — import and export | **10C-3**, re-owned by [12E-3c](phase-12e-layer-ops-status.md)'s decision E7 after it MEASURED the premise and found it unverified: 137 PSDs were scanned across the repo and `~/devel/rasterize-fixtures` and NONE contains a `cmls` block or image resource 1065; the Photoshop harness cannot author layer comps and `scripts/photoshop-probe/psd_records.py` exposes neither resource bodies nor a record's `shmd` items. Adobe's document defines Layer Comps as image resource **1065** (a version-16 descriptor) and leaves the per-layer rows to the undocumented `shmd` list, so both the 12E-3 contract's "comps naming layers by record index" premise and this doc's earlier "by a persisted id" are UNVERIFIED claims. 12E-3c therefore imported nothing and re-recorded the loss: the EVIDENCE family (a Photoshop-authored `comps` fixture plus a `comps.json` holding the resource-1065 hex and the per-record `shmd` item keys) must be authored FIRST, and it is the first item of 10C-3's criterion. Until then the import keeps no comp, and `rasterize_core.h` and the 12E status doc say so. |
| Persisted layer ids in `.rz` | **shipped** by [12E-1](phase-12e-layer-ops-status.md): the always-written `LIDS` table carries every id across a save. **Rebinding by that persisted id shipped in 12E-1b** (`LCMP` schema 2, both depths). |
| Remapping recorded POSITIONS through canvas geometry | **12E**. A comp recorded before a crop applies its offsets verbatim afterwards, which is what a layer's own stored offset does too. |
| Comp REORDERING | not scheduled. The list order is save order; ◀ ▶ and `update_layer_comp` work on it. |
| Recording position for groups and adjustments | not scheduled — see decision 4. |
| Locks at high depth | **shipped** by [12E-1b](phase-12e-layer-ops-status.md): `precision_lock` gives a typed record the same four bits, so typed `locked` is a real answer. |

## 12D-2 — view-only rotation and the synchronized second view. Shipped 2026-09-19

The acceptance criteria this section replaces were fixed by 12D-1 and are met
in full; what they asked for is below, beside what shipped, what was decided
along the way and what stays open with an owner.

### View rotation: the contract

| Question | Answer |
| --- | --- |
| What turns | The VIEW. `ImageCanvasView` keeps its document-sized bounds and its own flipped coordinate system; it is now a subview of an UNROTATED `CanvasHostView`, which is what the scroll view scrolls, and only its `frameCenterRotation` changes |
| Sign | Positive is CLOCKWISE on screen. `frameCenterRotation` is counterclockwise-positive in the SUPERVIEW's numbers and the host is flipped, so the same matrix reads as a clockwise turn. Measured, not assumed |
| Range | Normalized into (−180, 180]. 180 stays 180, −180 becomes 180, 370 reads back 10 |
| Who owns it | The WINDOW. Two views of one document hold two angles, and Sync Pan & Zoom deliberately does not carry rotation |
| What it writes | Nothing. No pixel, no `applyEdit`, no `updateChangeCount`, no history entry, no Action step, nothing a `save_copy` carries — the ruler/grid/proof-setup exemption |
| How a tool still lands | Every pointer site in the canvas already reads `convert(event.locationInWindow, from: nil)`, and AppKit resolves the rotation there. NO call site changed |
| At 0° | `ViewRotation.hostSize` and `canvasOrigin` are the IDENTITY, and `frameCenterRotation` is assigned only when it actually differs — so every number `get_view`, `set_view` and the Navigator report is bit-identical to 12C-2's. `verify_phase12c2_workspace.py` V1 and the `navigator-ui` probe case are green unchanged |

Three ways in, all the same one writer (`EditorViewController.setViewRotation`,
which keeps the canvas point under the well's centre fixed): View ▸ Rotate
View… (⌥⌘R — an angle field, a −180…180 slider, Reset, live preview, Cancel
restores), the Hand tool's ⌥-drag about the well's centre (⇧ snaps to 15°), and
the trackpad's two-finger rotate, which any tool answers.

### Rulers, grid, guides — the decision

**The ruler strips HIDE while a view is turned.** A ruler labels a straight
axis, and under a turned view its ticks would stand beside pixels they do not
measure. `rulersVisible` is `ToolOptionsStore.shared.view.rulers &&
viewRotation == 0`, so the PREFERENCE
is untouched and Reset View Rotation brings the strips straight back — the
`rotate-view-gesture-ui` case gates exactly that.

**The grid, the guides, the marquee, the smart guides, the transform handles
and the active-layer outline all rotate WITH the picture** and needed no code
at all: every one of them is drawn inside `ImageCanvasView.draw`, in the
canvas's own bounds, so the host transform carries them.

**The Navigator's proxy is the axis-aligned BOUNDING BOX of the turned well**,
not a turned quadrilateral, and `get_view.visible` says the same thing. It is
recorded rather than hidden: the panel draws a rectangle, and a document-space
answer to "what am I looking at" has to be one too. A rotated thumbnail is
listed as out of scope below.

**Interpolation** goes to `.high` while the view is turned: a turned view has
no pixel grid for nearest-neighbour to line up with.

### The second view: shared vs per view

A second view is a second `EditorWindowController` on the SAME `NSDocument` —
AppKit's own model — so there is one `doc`, one history and one undo stack by
construction, and `shouldCloseWindowController` already keeps the document open
while another view exists and prompts once on the last.

| Per VIEW (window) | Per DOCUMENT (shared by every view) |
| --- | --- |
| Zoom, scroll position, view rotation | The picture, the history and the undo stack |
| The Navigator and Info panels, the right panel's tab | Layer selection and the active layer |
| Canvas SESSIONS: text entry, Free Transform, the crop box, the Quick Mask buffer, a selection stroke (`commitPendingCanvasSessions` already walks every window) | The paint target and the channel eyes |
| The window frame and title (`<name>:1` / `<name>:2` while there is more than one) | The selection, the guides, the ruler origin, layer comps, Sync Pan & Zoom |

Cross-view mirroring rides a channel of its own,
`.imageDocumentViewTargetsDidChange`, posted by `notifyViewTargetsChanged()`
from `oldValue !=`-guarded `didSet`s on `layerSelection`, `planeEditTarget` and
`channelVisibility`. Reusing `.imageDocumentImageDidChange` was rejected and the
reason is worth keeping: it fires on every live drag tick and inside every
publication window, so hanging mirroring on it would have changed what undo,
redo, revert and a depth conversion deliver to a SINGLE-window document. The
new post is guarded three ways — more than one window controller, not
`isPublishingEditingState`, not `history.navigating` — so a one-window document
behaves exactly as it did before this slice. The receiving editor runs only
`setPaintTarget` / `restoreVisibility` / `activeLayerDidChange`, none of which
writes back a value that differs from the posted one, so the mirror terminates.

`releaseDisplayForWindowClose()` now also removes the editor's three
document-object observers. Before 12D-2 a closed window meant a closed
document; now it does not, and an editor AppKit still retains would otherwise
re-acquire `document.previewImage` on the next edit and run `zoomToFit`,
`beginCropSession` and `refreshNavigator` with no window. `second-view-ui`
holds the closed editor, strokes window 1 and requires its canvas to stay empty.

### Decisions

1. **The host view, not a rotated document view.** Rotating the scroll view's
   `documentView` itself would have turned scrolling, the hand pan and the clip
   view's own constraint with it. An unrotated host means the well still scrolls
   along the screen's axes and the canvas's bounds are still document pixels —
   which is why nothing else in the editor had to learn about rotation.
2. **⌥⌘R and ⌃⌥⌘R, and an AppKit matching rule measured on the way.** ⌘R, ⇧⌘R
   and ⌃⌘R are Rotate 90° CW/CCW and View ▸ Rulers, so Rotate View… takes ⌥⌘R.
   Reset View Rotation deliberately does NOT take ⌥⇧⌘R, and the reason is a
   measured AppKit behaviour rather than a preference: **`NSMenu
   .performKeyEquivalent` ignores an EXTRA Shift on a lower-case letter** — an
   item at ⌘R fires on a ⇧⌘R press, and an item at ⌥⌘R fires on an ⌥⇧⌘R press
   (measured directly against `NSMenu.performKeyEquivalent`, both ways).
   Reset View Rotation therefore takes ⌃⌥⌘R, which ⌥⌘R cannot claim.

   **RE-MEASURED, and CORRECTED, by [12E-1b](phase-12e-layer-ops-status.md).**
   The measurement above, and the inference this paragraph draws from it ("so
   a `X` / `⇧X` pair can never work: whichever comes first in the walk
   swallows the other"), were made with a press built by
   `NSEvent.keyEvent(…)` carrying a LOWER-CASE
   `charactersIgnoringModifiers` together with Shift in `modifierFlags` — a
   shape no keyboard delivers, because `charactersIgnoringModifiers` ignores
   every modifier EXCEPT Shift. 12E-1b measured the same presses in BOTH
   shapes (probe case `shortcut-shift-rule-ui`, M1–M7, table in the 12E status
   doc), the second of them translated from a `CGEvent` the way the window
   server translates a real press. **On the hardware shape the leniency does
   not exist: a key equivalent matches the chord EXACTLY, in both
   directions.** So an item at ⌘R does NOT fire on a hardware ⇧⌘R press, an
   `X` / `⇧X` pair DOES work, and the shipped Undo ⌘Z / Redo ⇧⌘Z, Save ⌘S /
   Save As ⇧⌘S and Export ⌘E / Merge Down ⇧⌘E pairs are not shadowed — a
   hardware-shaped ⇧⌘Z fires Edit ▸ **Redo** (M7). `ShortcutMenu.conflict`'s
   exact chord comparison is therefore the right predicate and stays as it is;
   the item is CLOSED, not carried. The choice of ⌃⌥⌘R for Reset View
   Rotation is not wrong — ⌥⇧⌘R is free either way — but its stated REASON was
   an artifact of the event shape, and this paragraph now says so rather than
   asserting a leniency that only probe-built events show.

   The same rule cost the 9H shortcut probe its arbitrary test chord: it
   assigned ⌥⇧⌘R to View ▸ Rulers to prove an assignment reaches the live item,
   the palette and the file and survives a relaunch, and ⌥⌘R swallows that
   press *as the probe builds it* — the probe presses through
   `NSEvent.keyEvent`, which is exactly the synthesized shape 12E-1b showed
   the leniency belongs to, so the move was still necessary for the case. The
   CASE's chord moved to ⌥⇧⌘K (both ⌥⌘K and ⌥⇧⌘K are free) with its header
   stating why; nothing it asserts changed. **No shipped shortcut was
   renegotiated.**

   The companion finding was recorded as a gap at the time: `ShortcutMenu.conflict`
   compares chords EXACTLY, so it "would not refuse" a user assigning ⌥⇧⌘R
   while ⌥⌘R is held. **12E-1b's measurement DISSOLVED that gap.** On the
   hardware shape the two chords are distinct and both fire their own item, so
   an exact comparison is the right predicate and `conflict` stays as it is
   (its doc comment records the measured rule). M7 uncovered no shipped-pair
   shadowing — a hardware-shaped ⇧⌘Z fires Edit ▸ Redo. Nothing is carried to
   12E-2; the gap-table row below says CLOSED for the same reason.
3. **New View and Sync Pan & Zoom take no chord.** Both are deliberate,
   occasional commands; a free ⌘ chord is worth more elsewhere.
4. **`sync_pan_zoom` is DOCUMENT-wide** and says so in its description. It is
   one switch meaning "these windows move together", not a per-window opinion,
   and it lives on `ImageDocument` beside `lastAppliedLayerComp` as VIEW
   bookkeeping: never in `EditingState`, never persisted, never undone.
5. **An omitted `view` is ALWAYS view 1**, the document's FIRST window
   controller — the same editor `AgentServer.editor(_:)` hands every other
   handler. `NSApp.keyWindow` is never consulted, because it is nil whenever the
   app is inactive, which is most of the time an agent is driving it.
6. **Ordinals are minted once and never re-used** inside a session, so a closed
   view's id does not come back on a different window. After closing view 2 the
   next New View is view 3, which `second-view-ui` asserts.
7. **The sheet previews live and has no Apply.** Rotating the view is a frame
   transform, not a render, and there is no document to put back; Cancel simply
   restores the angle the sheet opened on.

### Gates

`scripts/verify_phase12d2_views.py` (`HostApp`, a free port, a dummy API key
and a private directory for every preference library), gates fixed before
anything was measured:

- **V1** `set_view {rotation: 30}` and `get_view` both answer exactly `30.0`;
  `370 → 10.0`, `-180 → 180.0`, `180 → 180.0`; `rotation: "x"` and
  `fit` + `zoom` together are refused BY NAME; and across all of it the
  `render` SHA-256, the `list_history` length and
  `get_document.unsaved_changes` are unchanged.
- **V2** the MCP surface takes DOCUMENT coordinates, so the independent oracle
  is the IDENTITY: the same one-point 100 %-opacity hard `brush_stroke` on a
  fresh transparent layer of two twin documents — one at 0°, one at 30° — gives
  byte-identical `render` digests.
- **V3** `open_second_view`'s published schema offers no `view` — the ordinal
  is minted by the call — while `close_second_view`'s requires one;
  `open_second_view` answers `view: 2`, `list_views` lists two,
  `set_view {view: 2, zoom: 1}` reads back `1.0` while
  `set_view {view: 1, fit: true}` reads back below 1 on a 4000 × 3000 canvas, an
  unqualified `get_view` equals `get_view {view: 1}` field for field, an
  unqualified `set_view {zoom: 1.5}` leaves view 2 alone, and after one `fill`
  both views report the same `revision` and the same history length.
- **V5** (run immediately after V3, so the edit it walks back is V3's) one
  `undo` moves `list_history.cursor` back by exactly one and returns the render
  digest to its pre-edit value.
- **V4** `close_second_view {view: 2}` leaves `unsaved_changes` exactly as it
  was and `list_views` lists one; `close_second_view {view: 1}` is refused by
  name as the last view; a `fill` after the close still succeeds and
  `get_view {view: 1}.revision` advances by exactly one.
- **V6** with `sync_pan_zoom: true`, `set_view {view: 1, zoom: 2, center:
  {1000, 800}}` makes view 2 report zoom `2.0` exactly and a centre within
  0.5 px — the only tolerance in the file, because the receiving clip view
  constrains its own origin.
- **N1** the carried 12D-1 nit: `apply_layer_comp {name: " Hero "}` succeeds on
  a comp saved as `Hero`. `compNamed` now trims like `save_layer_comp` stores.

UI-source cases (`scripts/native-paint-probe/views.swift`, run by
`scripts/verify_phase9h_ui.py --require-current`, whose exit status is a gate):

- **`rotate-view-stroke-ui`** — a 400 × 300 byte document turned 30°: the same
  flattened bytes, nothing to undo, not edited, both ruler strips hidden. The
  case then computes, independently, H = hostCentre + R(30°)·(P − canvasCentre)
  with R = [[cos, −sin], [sin, cos]] in the flipped host space, converts it to a
  window point, requires the canvas to map it back to P = (120, 90) within
  0.01 px, presses the brush at that WINDOW point and requires the dab's
  alpha-weighted centroid to be P within 0.5 px.
- **`rotate-view-gesture-ui`** — the Hand tool's ⌥-drag: the angle the editor
  lands on equals the case's own clockwise-positive atan2 delta about the well's
  centre within 0.01°, no pixel/undo/dirty change, and Reset puts both the angle
  and the rulers back.
- **`rotate-view-sheet-ui`** — the Rotate View sheet's angle FIELD, which no
  MCP drive can reach: 45 typed into the field editor with no Return and no
  slider touched, then Done clicked, must be the angle the sheet previews and
  commits (a sheet reading its slider back at Done answers the old angle).
- **`second-view-ui`** — two real windows titled `<name>:1` / `<name>:2`; a
  stroke in window 1 leaves both canvases showing the same CGImage; a layer and
  paint-target change in window 1 reaches view 2's `paintTarget` and its layers
  panel's selected row; zooms 2 and 0.5 read back independently; an undo driven
  from window 2 moves the shared history cursor by one; `performClose` on
  window 2 leaves one window controller, the document open and its dirty flag
  unchanged; a further stroke leaves the CLOSED editor's canvas empty while the
  document still has a preview; and with Sync Pan & Zoom on, a zoom to 3 in
  window 1 reads 3 in a reopened second view.

### Open, with owners

| Gap | Owner |
| --- | --- |
| Destructive whole-canvas rotation by an arbitrary angle (gap review §3H) | **CLOSED at 8 bits by [12F-1b3](phase-12f-geometry-status.md) and at 16 bits by 12F-1b4**: Image ▸ Rotate Arbitrary…, MCP `rotate` with any finite `degrees` plus a `sampler`, and a recorded step, all through the shared `doc_rotate` classification/extent with an EXPANDING canvas; exact quarter turns delegate bit for bit and content-identical exact results are successful no-ops. 32-bit float keeps refusing a filtered angle by name, owner 11F's resampling oracle. This slice is §F14's VIEW rotation and touches no pixel. |
| A Rotate View TOOL in the tool rail | not scheduled. The command, the sheet, the ⌥-drag and the trackpad gesture cover the workflow; a rail slot would add an `EditorTool` case and every exhaustive switch with it. |
| Match Zoom / Match Location across DOCUMENTS | not scheduled. Sync Pan & Zoom is per document, which is what F14's "detail and composition simultaneously" asks for. |
| A rotated Navigator thumbnail and a turned proxy quadrilateral | not scheduled. Recorded above: the proxy and `get_view.visible` are bounding boxes. |
| Persisting rotation, the view count or Sync Pan & Zoom in `.rz`, a workspace layout or an Action | not scheduled, and deliberate: `WorkspaceLayout` stores neither zoom nor rotation for the same reason it never stored zoom — that is per document, per window and per session. |
| Text-session editing quality and hover-cursor tracking under a rotated view | hand-checked, not gated. `CanvasHostView.resetCursorRects` forwards the tool's cursor over the host's exposed corners while turned; the canvas keeps its own tracking area. |
| `ShortcutMenu.conflict` does not see AppKit's Shift leniency (decision 2) | **measured by [12E-1b](phase-12e-layer-ops-status.md) and CLOSED.** The probe (M1–M7, both event shapes) shows the leniency exists only for presses hand-built with `NSEvent.keyEvent`; on the shape the window server delivers a key equivalent matches the chord EXACTLY and a ⇧⌘Z press fires Edit ▸ Redo. `conflict`'s exact comparison is right and is untouched; the table is in the 12E status doc. |
| `set_view` accepted a malformed `zoom`, `center` or `view` argument (a string zoom, a non-object centre, a non-integer view) and answered `ok: true` with the view unchanged; `close_second_view`'s `view` description said "omit it" where it is required | **CLOSED in [12E-1](phase-12e-layer-ops-status.md).** Every present `zoom`, `center`, `fit`, `rotation` and `sync_pan_zoom` that fails to parse is refused BY NAME; `viewArgument(_:)` is the one reader of `view` and refuses a non-integer with "view must be an integer ordinal — call list_views."; `close_second_view` has its own `view` sentence and the public reference is regenerated. Drive gate S4. |

## Receipts

- 12D-1: `~/devel/rasterize-fixtures/phase12-receipts/12d/receipt.json`, written
  by `scripts/verify_phase12d1_comps.py` (gates C1–C4; the file records
  `"tolerances": "none"` and every gate's own readings).
- 12D-2: `~/devel/rasterize-fixtures/phase12-receipts/12d2/` — `receipt.json`
  from `scripts/verify_phase12d2_views.py` (V1–V6 + N1), `ui-probe.json` from
  `scripts/verify_phase9h_ui.py --require-current`, and the 12C-2 and 12D-1
  drives re-run under `12c2-rerun/` and `12d1-rerun/` to show this slice moved
  no number they own.
