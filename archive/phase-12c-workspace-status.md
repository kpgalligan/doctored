# Phase 12C — basic workspace productivity (12C-1 and 12C-2)

**Shipped 2026-09-18** on `codex-refactor`, as the FIRST of two bounded runs.
12C's plan entry lists five independent changes; the slice was split under the
request's own item 7:

- **12C-1 (this run):** New Document, layer labels / search / filter, and the
  shared preference-library lift, plus 12B-2's carried "library is full"
  wording nit.
- **12C-2 (shipped 2026-09-19):** Navigator, customizable shortcuts, saved
  workspace layouts, plus the two carried nits. Its own section is below and
  what it left open is at the end of this document.

## What ships

### 1. New Document (File ▸ New…, ⌥⌘N)

A document can now start with no seed image: width and height in `px`, `in`,
`cm` or `mm` at a PPI, a colour profile (sRGB, Display P3 or an RGB ICC file),
8 or 16 bits, and a background of transparent, white, black or a `#rrggbb`
colour. `NewDocument.swift` is the model half (`NewDocumentUnit`,
`NewDocumentBackground`, `NewDocumentProfile`, `NewDocumentRequest`,
`NewDocumentPreset`, `NewDocumentPresetLibrary`, `ImageDocument.makeNew`),
`NewDocumentSheetController.swift` the modal card, and
`AgentServer+NewDocument.swift` the MCP twin — `new_document`,
`list_document_presets`, `save_document_preset`, `delete_document_preset`.

The naming rule: a transparent document's one layer is **"Layer 1"**, a
filled one's is **"Background"**.

`makeNew` deliberately does NOT call `adoptWorkingSpace()`. The request already
names the space its numbers belong to; converting it into the app-wide
working-space preference would build a document in a profile the caller did not
ask for.

**Two refusals BY NAME, not silent substitutions.**

- **32-bit float** is refused. Phase 11F admits no promotion into float: a
  float document comes only from a floating-point TIFF or a v40 `.rz`, so
  offering the depth here would promise a mode the core cannot build. The
  sheet's depth popup omits it and the tool's `depth` enum is `[8, 16]`.
- **Gray** is refused (`RasterProfileKind.notRGB`). The core stores RGB
  documents only. A Gray or CMYK ICC path names its kind in the refusal.

### 2. Layer colour labels, search and filter

A per-entry colour label, 0 none and 1…7 exactly Photoshop's `lclr` palette
codes in Photoshop's own order (red, orange, yellow, green, blue, violet,
gray), so import and export carry the number itself with no mapping table.

- **Core:** `core/src/doc_label.rs` (`Layer.label`, `with_layer_label`,
  `layer_label`, the merge rule), `core/src/label_native.rs` (the `.rz`
  wrapper), `rz_doc_with_layer_label` / `rz_doc_layer_label`, the typed
  `{"op":"layer_label",…}` edit and the `label` inspection row.
- **App:** `LayerLabel.swift` (`LayerLabel`, `LayerKindFilter`, `LayerFilter`),
  the layers panel's filter bar, the eye-gutter tint, Layer ▸ Label and the row
  menu's Label ▸, `ActionSteps.setLayerLabel`, and
  `AgentServer+LayerLabels.swift` (`set_layer_label`, `find_layers`).
- `get_document` rows gain `label` at BOTH depths, spelled with the NAME —
  the typed `get_document` maps the core's numeric info row into the same
  vocabulary the byte rows use, so a caller never has to know which depth it
  is reading.

**A label is lock-exempt**, like a rename: it marks a row in the panel and
changes no pixel, so a locked layer can still be labelled. It is deliberately
not under `under_locks` and not in `structure_tests`' lock sweep.

**Carriage.** Duplicate copies it; group/ungroup, a deep document copy and
every geometry op carry it; a **merge keeps the LOWEST merged entry's label**
(`doc_identity`'s rule — the merged entry takes that member's slot, name,
locks, link and id); a **flatten** makes one new picture, so its entry is
unlabelled.

**The panel's filter** narrows the rows and never touches the document. While
a filter is active the panel walks `tree.visibleRows(collapsedHidden: false)`,
so a match inside a CLOSED group gets a row of its own with its depth indent
intact, and the `mappedToVisibleRows` remap is skipped so clicking such a row
selects that entry rather than its closed ancestor. Clearing the filter
restores both. The footer reads "Filtered N of M" while it is on.

### 3. `.rz` and PSD carriage

`RZDC_VERSION` moves 40 → **41 (byte) / 42 (typed)**, as one more conditional
OUTERMOST wrapper on `evaluation_native.rs`'s shape:

```
byte    41 → 38 → 28 → body/tails → MOUT → EVAL → LLBL
typed   42 → 40 → 39 → 36 → 35 → 32 → 29 → 37 → body/tails → … → EVAL → FLOT → LLBL
```

`LLBL` is `u32 schema = 1`, `u32 count ≥ 1`, then count pairs of `u32` index
(strictly ascending) and `u32` code (1…7). The byte index is the bottom-first
layer index (`MOUT`'s convention); the typed index is the postorder record
index (`LCLP`'s). **The table is written only when some entry carries a label**,
so an unlabelled document's bytes are byte-for-byte what the 40/38 writer made,
first word included — and releasing the last label restores them exactly. Code
0, an empty or non-canonical table, a code above 7 and a trailing word all
refuse BEFORE a single PNG is decoded.

The disclosure: a v41/v42 file is REFUSED by an older build, exactly as v38/v40
files are.

A layered PSD carries labels in `lclr` in BOTH directions at BOTH depths. An
`lclr` body of any length other than 8, or a code above 7, is dropped to 0
rather than refusing the file: a swatch is cosmetic. The `psd_10c` goldens are
unchanged — no fixture carries a label.

### 4. The shared preference-library contract

12B-2's per-entry diagnosis is lifted into `app/Sources/PreferenceLibrary.swift`
and every library now speaks it:

```swift
protocol PreferenceLibraryCodec { associatedtype Item; noun, plural,
    directoryEnvironment, directoryName, maximumEntries, maximumFileBytes,
    schemaVersions, didChange; name(of:), json(of:), decode(_:schemaVersion:) }
enum PreferenceLibrary<Codec> { directory, normalizedName, validatedFileName,
    locked, readBytes, diagnose, readListing(repairable:), listing, entries,
    named, save(_:replace:), delete(name:), delete(file:) }
```

Codecs: `RawPresetCodec` (128 × 8 KiB, schemas 1…2), `LiveFilterPresetCodec`
(128 × 8 KiB, 1…1), `ExportPresetCodec` (64 × 4 KiB, 1…2),
`MetadataPolicyCodec` (64 × 4 KiB, 1…1) and the new `NewDocumentPresetCodec`
(64 × 4 KiB, 1…1, `RZ_DOCUMENT_PRESETS_DIR`, `Rasterize/Document Presets`).

Every sentence is built from the codec's `noun`, so two libraries cannot word
one refusal two ways. `list_live_filter_presets`, `list_export_presets`,
`list_metadata_policies` and `list_document_presets` all report `broken` rows
now, and their delete tools take exactly one of `name` or `file` — whole-library
strictness is gone from the filter, export and metadata libraries.

**Repair stays RAW-only.** Only RAW has a field-by-field recovery to run; the
shared listing takes a caller's dry run as the `repairable` flag and answers
`false` everywhere else. `RawPresetLibrary` keeps `Fault`, `Broken`, `Entry`,
`Listing` and `repair(file:)` as wrappers over the shared types, so every
existing caller's signature survives unchanged.

The carried nit is closed: the full-library refusal now reads **"The RAW preset
library folder holds 128 files; delete a preset or a broken file first."** —
which says what is counted and what frees it, and still contains `128`, so
`scripts/verify_phase12b2_presets.py`'s P8 assertion needed no edit.

## Decisions

**⌘N stays New from Clipboard; File ▸ New… takes ⌥⌘N.** A shortcut already in
the user's fingers is never silently renegotiated (`photo-editor-gap-review.md`
§3J, the same rule that put the command palette on ⇧⌘K because ⌘K is Crop).
⌘N and ⇧⌘N were the only taken `N` combinations, so ⌥⌘N was free.

**Recordability.** `set_layer_label` IS an Action step (per-document, undoable,
`.rz`-persisted), with the `layers` set form `set_layer_lock` established.
`new_document` and `find_layers` are `nonRecordable` — the first for
`open_document`'s reason (a replay would pop a second window beside the run's
real target), the second because it only reads. The three document-preset tools
are `nonRecordable` AND `notAStepTool`: library writes are preferences, outside
document Undo. `new_document` is deliberately NOT `notAStepTool` — an action
MAY create a document on purpose, exactly as `open_document` may.

**Depth admission.** All six tools are in `DocumentPrecision.commonTools` and
`FloatDocument.tools`: a label is metadata (a float document admits
`layer_name` for the same reason), `find_layers` and `new_document` only read
or create, and the preset tools write a preference library.

**`set_layer_label`'s reply is its own shape**, not `setLayerLock`'s
`structureNoOp`: entries WRITTEN come back in `layers`, entries already at that
label in `skipped`, and `changed` says whether an undo step exists at all. When
every entry is skipped, nothing is pushed — no document handle, no undo step —
which is the core's purity rule reported honestly rather than as a refusal.

## Verification

```
cd core && cargo fmt --check
make lint && make test
cd core && cargo test --release --test layer_label
make typecheck && make app
python3 scripts/verify_phase12c_workspace.py --app … --receipts ~/devel/rasterize-fixtures/phase12-receipts/12c
python3 scripts/verify_phase12b2_presets.py --app … --receipts ~/devel/rasterize-fixtures/phase12-receipts/12c/12b2-rerun
python3 scripts/generate_mcp_reference.py --url … --output … --check
```

`core/tests/layer_label.rs` (12 tests, independent references, through the
FFI): the setter's refusals and its no-op rule; duplicate/merge/flatten,
group/ungroup and a document copy carrying labels; the byte and typed `.rz`
round trips against an `LLBL` table the test assembles itself; an unlabelled
document's bytes restored exactly when the last label is released; the byte
wrapper nesting outside `MOUT` and `EVAL`; a labelled FLOAT document writing
`LLBL` after `FLOT`; five crafted tables refused by name; and `lclr` in both
directions at both depths, read back by the test's own record walk.

`scripts/verify_phase12c_workspace.py` — five gates, all PASS:

- **N1** 8 × 10 in @ 300 ppi Display P3 16-bit transparent → exactly
  2400 × 3000, `precision == "rgba16"`, one layer named "Layer 1", alpha 0 at
  five points, and an exported PNG whose ICC (extracted with `magick`, parsed
  in the drive from the raw tag table) has description "Display P3" and
  `rXYZ`/`gXYZ`/`bXYZ` equal to the system Display P3 profile's to within the
  s15Fixed16 step. A 100 × 100 white 8-bit document's layer is "Background"
  with an opaque white centre. A saved preset replays to the same summary.
  `depth: 32` and a Gray ICC path are refused by name.
- **L1** a raster + group + adjustment document with three labels survives
  `save_copy` `.rz` + `open_document` and `export_psd` + `open_document`
  (layered) at 8 AND 16 bits, and again at 8 bits with
  `set_evaluation linear_light` on.
- **L2** a 60-entry document with a labelled child inside a CLOSED group:
  `find_layers` by name, by each of the six kinds, by label and combined
  returns exactly the index set the drive computes for itself.
- **L3** a recorded `set_layer_label` replays through `run_action`; a repeat
  answers `changed: false` with the index in `skipped` and adds no history
  state.
- **S1** the document and filter libraries each list their valid entry beside
  exactly one `not_json` `broken` row, `delete_*_preset {file}` clears it, and
  a save onto an occupied canonical slot is refused naming the file.

The 12B-2 drive re-runs unchanged: all nine gates PASS.

Receipts: `~/devel/rasterize-fixtures/phase12-receipts/12c/receipt.json` and
`…/12c/12b2-rerun/receipt.json`; the post-review re-run's own copies are in
`…/12c/fix-20260919T0130/` (both drives re-run after the review fixes, all five
and all nine gates PASS again).

### The panel check — measured through the UI-source probe

The filter bar has no MCP twin: the filter changes no document, so it takes the
same view-preference exemption rulers, the grid and the snap toggles already
have (`app/CLAUDE.md`).

**Covered automatically.** The MATCHING half is `LayerFilter.matches`, and the
panel and `find_layers` call that one function — L2 drives it over a 60-entry
document by name, by each of the six kinds, by label and combined, and asserts
the closed group's labelled child IS in the answer.

**Covered by the UI-SOURCE probe — measured, not claimed.** The ROW-WALK half
has no MCP twin, so it is qualified the way every other gesture-only behaviour
is: a new case in `scripts/native-paint-probe/panel.swift`
(`label-filter-panel-ui`, registered in that probe's `main.swift`) compiled
against the ACTUAL `app/Sources` by `scripts/verify_phase9h_ui.py
--require-current`. It builds a real 8-bit document in a real window, groups a
layer called "Sky" into a folder, labels the child `red` through
`set_layer_label`, closes the folder through `set_layer_properties {open:
false}`, and then asserts, on the real `LayersPanelViewController`:

| Assertion | Measured |
|---|---|
| unfiltered rows (child has NO row) | `[2, 0]` — folder 2 and the background; child 1 absent |
| filter by Red, chosen through the panel's own `All Labels` popup | `filter.label == .red`, rows `[1]` — the child inside the CLOSED group |
| the row keeps its OWN indent | `rows[0].depth == 1`, the child's tree depth, not the folder's 0 |
| the document is untouched | layer count unchanged, `layerOpen(folder) == false` |
| clicking that row selects the CHILD | `activeLayerIndex == 1` (the child), not 2 (the closed ancestor) |
| clearing the filter restores the collapsed walk | rows back to `[2, 0]`, child has no row, selection remapped to folder 2 |

Run of 2026-09-19: `PASS label-filter-panel-ui`. Receipts (report, host report,
a screenshot of the filtered panel, the per-case list) in
`~/devel/rasterize-fixtures/phase12-receipts/12c/panel-probe-20260919T012338/`.

Scope, stated rather than glossed: the popup is driven by picking its title and
firing the control's own action, and the click is the real table taking the row
followed by the panel's own `tableViewSelectionDidChange` — AppKit delivers the
table's selection notification on its own schedule, which a pumped probe cannot
depend on. These are synthetic events through the real handlers, the same claim
`scripts/native-paint-probe` already makes everywhere else; no hardware input is
asserted.

**The rest of that probe's run was NOT green in 12C-1, and that was
pre-existing.** Six cases — `pressure-ui`, `airbrush-ui`, `pixel-airbrush-ui`,
`zero-pressure-pixel-ui`, `quick-mask-ui`, `byte-pressure-ui` — failed with
"Tablet conversion changed owned canvas coordinates", the NSEvent→CGEvent→
NSEvent round trip drifting on this machine, inside `send()`'s own coordinate
assertion before any app code was reached. **12C-2 fixed it** (see "The tablet
drift, measured and fixed" below): all sixteen pass-1 cases and the one
pass-2 case are green, and `verify_phase9h_ui.py` exits 0.

## Files

Rust: `doc.rs` (the field only), `doc_label.rs` and `label_native.rs` (new),
`document_clone.rs`, `doc_group.rs`, `doc_group_ops.rs`, `doc_structure.rs`,
`rzdc.rs`, `rzdc_read.rs`, `precision_document.rs`, `precision_structure.rs`,
`precision_document_edit.rs`, `precision_retouch_output.rs`,
`precision_native.rs`, `precision_native_read.rs`, `precision_native_plan.rs`,
`precision_inspect.rs`, `precision_capability.rs`, `float_document.rs`,
`ffi_precision_edit.rs`, `ffi_doc.rs`, `psd.rs`, `psd_group_records.rs`,
`psd_import16.rs`, `psd_export.rs`, `psd_export16.rs`,
`include/rasterize_core.h`, `tests/layer_label.rs` (new).

Swift: `PreferenceLibrary.swift`, `LayerLabel.swift`, `NewDocument.swift`,
`NewDocumentSheetController.swift`, `AgentServer+LayerLabels.swift`,
`AgentServer+NewDocument.swift`, `EditorViewController+LayerLabels.swift` (all
new); `RawPresetLibrary.swift`, `LiveFilterPresetLibrary.swift`,
`ExportWorkspace.swift`, `MetadataPolicy.swift`, `RasterCore.swift`,
`RasterDocument+PrecisionMetadata.swift`, `LayerTree.swift`,
`LayerCellView.swift`, `LayersPanelViewController.swift`,
`LayersPanelViewController+Rows.swift`, `EditorViewController.swift`,
`AgentServer.swift`, `AgentServer+Precision.swift`, `AgentCatalog.swift`,
`AgentServer+FilterPresets.swift`, `AgentServer+Export.swift`,
`AgentServer+ProductionOutputs.swift`, `ActionSteps.swift`, `Action.swift`,
`DocumentPrecision.swift`, `FloatDocument.swift`, `AppDelegate.swift`,
`WelcomeWindowController.swift`.

Scripts/docs: `scripts/verify_phase12c_workspace.py` (new),
`scripts/host-source-probe/support.py`, this document, the plan,
`designs/phase-12b-actions-status.md`, `README.md`, `app/CLAUDE.md`,
`core/CLAUDE.md`, `public-docs/src/content/docs/reference/mcp-tools.md`.

# 12C-2 — Navigator, customizable shortcuts, saved workspaces

**Shipped 2026-09-19** on `codex-refactor`, the SECOND of the two bounded runs,
with 12C-1's two carried nits closed.

## What ships (12C-2)

### 5. The Navigator (View ▸ Navigator, ⌃⌘N)

A fifth right-panel tab beside Layers / Channels / Assistant / Info: the whole
composite as a thumbnail with a proxy rectangle over what the well is showing,
drag-to-pan inside the proxy, click-to-centre outside it, a log2 zoom slider, a
percentage field and Fit. `NavigatorPanelViewController.swift` holds the view
and the panel; `EditorViewController+Navigator.swift` holds `EditorViewState`,
`viewState` / `applyViewState` and `scrollCanvasCenter`.

**Nothing here writes a document.** The thumbnail is READ from
`ImageCanvasView.image` and never assigned to it (that setter clears the
overlay, the selection and the Quick Mask); the proxy is read from the scroll
view; the gestures scroll and magnify. `get_view` / `set_view`
(`AgentServer+Workspace.swift`) are the MCP twins, `nonRecordable` on the
proof-setup precedent. Zoom is refused outside 0.02…32 and a centre outside the
canvas is refused BY NAME rather than clamped, and the reply is the READ-BACK
state.

The thumbnail is cached against the CGImage identity it was built from and
rebuilt only when the panel is visible and the change is not live: a paint
stroke posts its change on every mouse-moved event, and re-scaling a
canvas-sized image per tick is the per-event canvas-sized read the Info panel's
memo already exists to avoid.

### 6. The right panel is draggable, and its width is a preference

`DS.panelWidthRange` is 240…560 and `ViewToolOptions.panelWidth` (default 304,
the old `DS.panelWidth`) carries it. `PanelDividerView` (`EditorChrome.swift`)
replaces the 1 px `NSBox`: it is `DS.panelDividerWidth` (7 pt) wide, draws the
same 1 px line at x = 3, overlaps the well and the panel by 3 pt each and is
added to the root LAST so it hit-tests ahead of both. A 1 pt divider is not a
target; the two `+ 3` constants keep the visible line, the panel's left edge
and `EditorWindowController`'s `+ 1` term exactly where they were.

One stored `panelWidthConstraint` on the Layers panel drives all five panels
(the other four are `equalTo: panelView.widthAnchor`). It is created in
`loadView` from the STORE — the `updateRulerVisibility()`-inside-`installRulers`
pattern, because `applyViewChrome()` only runs on the change notification — and
`applyViewChrome()` pushes the store value on every chrome change in every
window, so there is exactly one change path. `EditorWindowController` sums the
store value too, so a second window's 1:1-vs-fit decision sees the real well.

`AssistantPanelViewController`'s API-key field was pinned to
`DS.panelWidth - 24`, which becomes UNSATISFIABLE below 304 pt; it follows its
own box now. **No panel subview may pin a width to `DS.panelWidth` any more**,
and the probe gates a 240 pt layout with no overflow.

### 7. Customizable shortcuts (Rasterize ▸ Settings…, ⌘,)

`Shortcuts.swift` holds `ShortcutChord`, `ShortcutBindings`, `ShortcutCodec`
(on the shared `PreferenceLibrary` contract, 4 × 32 KiB under
`RZ_SHORTCUTS_DIR`, schema 1) and `ShortcutMenu` — the menu walk, the
launch-time defaults snapshot, apply, conflict and reset.
`ShortcutsSheetController.swift` is the sheet (search field, a path | chord
table, a `ShortcutRecorderField`, per-row Reset, Reset All and an inline
refusal line) and `AppDelegate+Shortcuts.swift` the two-step menu build.

**Identity is the SELECTOR** — `NSStringFromSelector(action)` plus `#tag` when
the tag is non-zero — never the title: a binding keyed on "Rotate 90°
Clockwise" is orphaned the day that item is renamed or localised.

**The defaults are a launch-time SNAPSHOT of the menu `buildMainMenu()`
produced**, and the file holds only the user's overrides. That is what makes
"existing shortcuts are never silently renegotiated" structural rather than a
promise: `Reset All` restores exactly what this build ships, and only an
override can change a binding. `makeMainMenu()` = build → `installStored`,
applied to the LIVE `NSMenuItem`s, so the command palette needs no change —
`CommandPalette.commands()` re-walks live items, and `shortcutText(for:)` now
forwards to a `shortcutText(key:modifiers:)` `ShortcutChord.display` shares.

**A chord must contain ⌘ or be F1–F19**; a menu key equivalent is resolved
AHEAD of the first responder (the Edit ▸ Clear ⌫ hazard), so anything else
would be taken from every text field in the app. **Conflicts are checked
against the WHOLE walk** — the app menu, the system selectors and hidden
alternates included — and refused naming the holder's path ("⌘K is already
Image ▸ Crop."). Not customizable: the app menu, `ShortcutMenu.systemSelectors`
(Quit, Hide, Close, Minimize, Undo/Redo, About, Settings…, Help), Open Recent,
the dynamic Workspaces and Delete lists (anything with a `representedObject` —
a binding on a row that may not exist tomorrow), and hidden/alternate items, so
the hidden ⌘= Zoom In keeps its default. A broken library file applies NOTHING
and says so in the sheet's message line.

### 8. Saved workspace layouts (Window ▸ Workspaces)

`Workspace.swift` holds `EditorPanelTab`, `WorkspaceLayout` (schema 1) and
`WorkspaceCodec` (32 × 4 KiB under `RZ_WORKSPACES_DIR`);
`EditorViewController+Workspace.swift` holds `captureWorkspace`,
`applyWorkspace` and `setPanelWidth`; `WorkspaceSheets.swift` the name sheet;
`AppDelegate+Workspaces.swift` the menu, rebuilt in `menuNeedsUpdate` because
another instance of the app can write the folder. A layout carries the window
frame, the panel's visibility, tab and width, the rulers/grid/snap toggles and
the current tool.

**The tab is restored through the EXISTING entry points**, never a bare
`panelTab = …`: `showInfoTab()` is the only path that installs
`infoPanel.selectionProvider` and seeds the readout, and `showNavigatorTab()`
the only one that pushes the first thumbnail. A restored Info tab that looked
right and measured nothing is exactly the bug this rule prevents, and the probe
asserts the provider.

The window frame goes through `NSWindow.constrainFrameRect(_:to:)`, so a layout
saved on a larger or second display comes back reachable rather than
off-screen. A tool the document's depth refuses is SKIPPED, not beeped:
restoring a layout on a 16-bit document is not the moment to say the Brush is
unavailable. The UI's Save writes `replace: true` (its sheet already asked for
the name); MCP's `save_workspace` defaults to `replace: false`, the preset
convention.

### 9. The two carried nits

`NewDocumentProfile.data` is `data() throws` now: an unreadable or empty
profile PATH is refused with "Could not read the ICC profile at <path>." rather
than yielding empty `Data` and depending on `assigningProfile` rejecting it.
`NewDocumentProfile.init(name:)` names the path the same way, and the size/kind
refusals are unchanged. The 9H probe's exit status is honest and green (below).

## Decisions (12C-2)

**Where the Navigator lives: a right-panel TAB, index 4, beside Info** — not a
floating panel. Every other inspector in this app is a tab in that panel, a
floating panel would need its own window controller, frame persistence and
close/reopen story, and "detail AND composition simultaneously" is 12D's second
VIEW, not a second panel. ⌃⌘N follows the View menu's own convention (Channels
⌃⌘C, Assistant ⌃⌘A, Info ⌃⌘I) and was free.

**A workspace layout does NOT store the document's zoom or scroll.** A layout
is window furniture and applies to whatever document is in front of you; where
you are looking inside one picture is per document and belongs to
`EditorViewState` (`get_view` / `set_view`). Storing zoom would make restoring
a layout jump every open document to a magnification chosen while looking at a
different one.

**The shortcut file is one item named `"Shortcuts"`** in a four-file library
rather than a bare JSON file: a stray copy is then a diagnosed `broken` row
instead of a refusal of the whole folder, and the lock, the bounded reader, the
atomic publish and delete-by-file come from `PreferenceLibrary` for free.

**Recordability.** `get_view` and `set_view` are `nonRecordable` (view state is
per window: no undo step, no dirty flag, nothing a `save_copy` carries — the
proof-setup exemption). The four workspace tools are `nonRecordable` AND
`notAStepTool`: a step naming a mutable library entry would not replay the
layout it was authored against. All six are in `DocumentPrecision.commonTools`
and `FloatDocument.tools` — none of them touches pixel storage.

**No MCP twin for the shortcuts** or for the panel filter bar: both are
app-wide preferences with no `document_id` that would not be a lie, no undo
step that could walk them back and nothing a `save_copy` carries — the view
preference exemption `app/CLAUDE.md` states. `restore_workspace` is the one
tool that changes rulers/grid/snap/tab, and it does so as a LAYOUT.

## Verification (12C-2)

```
cd core && cargo fmt --check && cd .. && make lint && make test
make typecheck && make app
export PYTHONDONTWRITEBYTECODE=1 R=~/devel/rasterize-fixtures/phase12-receipts/12c2
python3 scripts/verify_phase12c2_workspace.py --app build/Build/Products/Release/Rasterize.app --receipts $R
python3 scripts/verify_phase9h_ui.py --require-current --report $R/ui-probe/report.json
python3 scripts/verify_phase12c_workspace.py --app build/Build/Products/Release/Rasterize.app --receipts $R/12c1-rerun
python3 scripts/generate_mcp_reference.py --url http://127.0.0.1:4917/mcp --output … --check
```

`verify_phase12c2_workspace.py` (`HostApp`, private directories for all seven
libraries, tolerances fixed before measuring — there are none):

| Gate | Measured |
|---|---|
| **V1** `set_view {zoom: 4, center: {800, 600}}` on a 1600 × 1200 document | reply `zoom 4`, `center {800, 600}`, `visible {604.75, 468.875, 390.5 × 262.25}` — equal to `get_view` and to the request, exactly |
| V1 nothing written | `render` SHA-256 `65d9366a…` before and after, `list_history` 1 state before and after, `unsaved_changes` unchanged (a `new_document` is born dirty on purpose, so "unchanged" is the honest question) |
| V1 refusals | `zoom: 100` and a centre outside the canvas refused by name; `set_view` with neither refused |
| **W1** `restore_workspace B` | `applied` equals the seeded layout field for field, window frame included |
| W1 re-capture | `save_workspace C` == B's numbers; `restore_workspace A` then `save_workspace D` == A's numbers |
| W1 library | `list_workspaces` → A, B, C, D plus exactly one `not_json` broken row; `delete_workspace {file}` clears it; `{name: "B"}` removes B; both/neither refused; `save_workspace A` without `replace` refused |
| **N2** unreadable ICC | "Could not read the ICC profile at …/work/missing.icc." |

B's 900 × 700 frame is anchored on the window's own frame rather than at a
literal (80, 80): `applyWorkspace` runs it through `constrainFrameRect`, and on
a machine whose display does not start at the global origin that CLAMPS the
window — correct behaviour, and not something a gate should call a failure. The
size, the panel, the chrome and the tool stay exactly what the drive chose.

### The UI-source probe — four new cases, and the tablet drift measured and fixed

`scripts/native-paint-probe/workspace.swift` adds four cases, and
`main.swift` gains an `RZ_PAINT_UI_CASES` filter plus a `secondPassOnly` flag;
`verify_phase9h_ui.py` now runs a SECOND process over the same private
directories with `RZ_PAINT_UI_CASES=shortcut-relaunch-ui` and asserts both host
reports, and sets all seven `RZ_*_DIR` overrides (the shortcut case WRITES a
library).

| Case | Measured |
|---|---|
| `navigator-ui` | on a 400 × 300 byte document the proxy equals this file's own map of `contentView.bounds ∩ canvas` at 100 %, 200 % and 400 % within 0.5 pt; a 30 pt proxy drag moves the clip origin; the document HANDLE, the flattened bytes, `canUndo == false` and `isDocumentEdited == false` are all unchanged |
| `workspace-restore-ui` | frame, `panelTab == 4`, the Navigator laid out at 420 pt, the store flags, `currentTool == .brush` and `captureWorkspace == B`; a SECOND window opened while B is applied also lays out at 420; `tab: info` restores `infoPanel.selectionProvider != nil`; `tab: assistant, width: 240` lays out at 240 with NO visible subview past 240.5 pt; applying A round-trips |
| `shortcut-edit-ui` | `newDocument:` is ⌥⌘N and `newFromClipboard:` is ⌘N, no app-menu item is offered; ⌘K on Zoom In is refused with "Image ▸ Crop" and changes nothing; a chord with no ⌘ is refused; ⌥⇧⌘R reaches the menu item, the command palette's row and the file; `resetAll` leaves every chord at its default and no overrides; the sheet lists exactly `customizableItems.count` rows |
| `shortcut-relaunch-ui` (pass 2) | a second process's menu carries ⌥⇧⌘R off disk, and `NSApp.mainMenu.performKeyEquivalent` on a ⌥⇧⌘R key-down returns true and flips `ToolOptionsStore.shared.view.rulers` |

**The tablet drift, measured and fixed.** The round trip is AFFINE, not the
constant translation it looked like: each correction lands about 98 % of the
way, so the residual falls by ~50× per attempt (measured: 2.94 → 0.0597 →
0.00120 → 2.47e-5 → …). `send()` now measures the offset in window
coordinates, rebuilds the tablet copy with `cg.location` shifted by its
negative (y flipped for CG screen space) and iterates until the drift is
EXACTLY zero — nine attempts in practice — stopping early when an attempt stops
improving. Exactly zero matters: `airbrush-ui` compares deposited dab centres
with `==`, not with a tolerance. The compensation runs BEFORE the old bounded
wait and pumps nothing; the wait remains as the fallback for a window the
window server is still digesting, because pumping for five seconds let the
airbrush's real `Timer` deposit dabs between a case's read and its mouse-up.
The `drift < 0.01` assertion and the six tablet cases' own assertions are
UNCHANGED.

Run of 2026-09-19: pass 1 `passed: true`, `failures: []`, 16 groups —
`pressure-ui`, `airbrush-ui`, `pixel-airbrush-ui`, `zero-pressure-pixel-ui`,
`versioned-mcp`, `quick-mask-ui`, `quick-mask-cancel-stale`,
`extended-tools-ui`, `excluded-seed-fill-ui`, `byte-pressure-ui`,
`byte-airbrush-ui`, `move-record-ui`, `label-filter-panel-ui`, `navigator-ui`,
`workspace-restore-ui`, `shortcut-edit-ui`; pass 2 `passed: true`,
`failures: []`, `shortcut-relaunch-ui`. The script exits 0.

Scope, stated rather than glossed: the two shortcut cases install
`AppDelegate().makeMainMenu()` as `NSApp.mainMenu` and restore the previous
menu in a `defer` (the probe has no app delegate, and
`CommandPalette.commands()` returns `[]` without a main menu). Both point the
Rulers item at the real editor for the walk and the key press, because
enablement for a NIL-target item is resolved through the KEY window's responder
chain and an `.accessory` probe that never becomes the active app has none —
the same "AppKit's own scheduling is not something a pumped probe can depend
on" concession `label-filter-panel-ui` already makes. The chord, the matching
and the command that runs are AppKit's own. `charactersIgnoringModifiers` is
`"r"`, not `"R"`: with Command held macOS drops the Shift transformation too,
which is why every menu in this app spells its key equivalent lower-case with
Shift in the MASK.

Receipts: `~/devel/rasterize-fixtures/phase12-receipts/12c2/` —
`receipt.json` (the MCP drive), `ui-probe/report.json`,
`ui-probe/host-report.json`, `ui-probe/host-report-pass2.json`,
`ui-probe/run.log`, `ui-probe/run-pass2.log`, the three owned screenshots, and
`12c1-rerun/receipt.json` (12C-1's five gates re-run green on this build).

## Files (12C-2)

No Rust change.

Swift, new: `NavigatorPanelViewController.swift`,
`EditorViewController+Navigator.swift`, `Shortcuts.swift`,
`ShortcutsSheetController.swift`, `AppDelegate+Shortcuts.swift`,
`Workspace.swift`, `EditorViewController+Workspace.swift`,
`WorkspaceSheets.swift`, `AppDelegate+Workspaces.swift`,
`AgentServer+Workspace.swift`.

Swift, modified: `EditorViewController.swift` (wiring only),
`EditorWindowController.swift`, `EditorViewController+Rulers.swift`,
`EditorChrome.swift`, `Theme.swift`, `ToolOptions.swift`, `Tools.swift`,
`LayersPanelViewController.swift`, `ChannelsPanelViewController.swift`,
`AssistantPanelViewController.swift`, `InfoPanelViewController.swift`,
`CommandPalette.swift`, `AppDelegate.swift`, `AgentServer.swift`,
`AgentCatalog.swift`, `ActionSteps.swift`, `Action.swift`,
`DocumentPrecision.swift`, `FloatDocument.swift`, `NewDocument.swift`.

Scripts/docs: `scripts/native-paint-probe/workspace.swift` (new),
`scripts/native-paint-probe/main.swift`, `scripts/verify_phase9h_ui.py`,
`scripts/host-source-probe/support.py`,
`scripts/verify_phase12c2_workspace.py` (new), this document, the plan,
`README.md`, `app/CLAUDE.md`,
`public-docs/src/content/docs/reference/mcp-tools.md`.

## Open after 12C-2 — with owners

- **A second synchronized view** ("detail AND composition simultaneously") and
  **layer comps** — 12D, as planned. 12C's Navigator satisfies "inspect
  composition" only.
- **A generic repair for the non-RAW libraries** — no owner yet: it needs a
  per-library field-by-field recovery that only RAW has today. Broken rows in
  the shortcut and workspace SHEETS stop at a message line for the same reason.
- **Bare tool-key remapping** (the canvas `keyDown` letters), **per-document
  shortcut sets** and **importing Photoshop `.kys` files** — out of scope here
  and unowned; the stored file's schema takes a second item without a version
  bump if a later phase wants a named set.
- **Workspace-per-document auto-restore** and a window frame that survives a
  move to a second display beyond `constrainFrameRect` — unowned.
- Still open from 12C-1: 32-bit float and Gray New Document (refused by name),
  broken-file rows in the filter/export/metadata SHEETS, and Photoshop `lclr`
  on adjustment RECORDS at import.
