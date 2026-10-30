# Workspace productivity: New Document, labels, preference libraries, Navigator, shortcuts, workspaces (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12C-1 shipped 2026-09-18, 12C-2 shipped 2026-09-19
- **Original:** [archive/phase-12c-workspace-status.md](../archive/phase-12c-workspace-status.md) — full history, measurements, receipts, drive gates N1/L1–L3/S1/V1/W1/N2 and UI-probe cases
- **Key commits:** `bc3c0fc` (12C-1), `8692207` (12C-2)

## Context
Basic workspace features were missing: starting a blank document, colour-labelling and filtering layers,
a navigator thumbnail, rebinding shortcuts and saving panel layouts. The five changes are independent and were
delivered in two bounded runs. 12C-1 also generalized 12B-2's per-entry RAW preset diagnosis to every preference
library.

## Decisions
- **New Document (File ▸ New…, ⌥⌘N)** — `../app/Sources/NewDocument.swift` (`NewDocumentRequest`,
  `NewDocumentPresetLibrary`, `ImageDocument.makeNew`), `NewDocumentSheetController.swift`, MCP
  `new_document` / `list_document_presets` / `save_document_preset` / `delete_document_preset`. Units
  px/in/cm/mm at a PPI; sRGB, Display P3 or an RGB ICC file; 8 or 16 bits; transparent/white/black/#rrggbb.
  A transparent document's layer is "Layer 1", a filled one's "Background". `makeNew` does NOT call
  `adoptWorkingSpace()` — the request already names its space. **32-bit float and Gray are refused by name**
  (no promotion into float exists; the core stores RGB only). An unreadable ICC path is refused naming the path
  (`NewDocumentProfile.data() throws`). The card's Save Preset… (a nested name card, the export-preset save
  sheet's shape) and Delete Preset (a listed preset, no confirmation) are the UI twins of `save_document_preset` and
  `delete_document_preset {name}`.
- **⌘N stays New from Clipboard** — an existing shortcut is never silently renegotiated; ⌥⌘N was free.
- **Layer colour labels**: 0 none, 1…7 exactly Photoshop's `lclr` codes in Photoshop's order, so PSD carries
  the number with no mapping. Core `../core/src/doc_label.rs`, `../core/src/label_native.rs`,
  `rz_doc_with_layer_label` / `rz_doc_layer_label`, typed `{"op":"layer_label"}`; app `LayerLabel.swift`,
  `AgentServer+LayerLabels.swift` (`set_layer_label`, `find_layers`); `label` (by NAME) on `get_document`
  rows at both depths. A label is **lock-exempt** (like rename). Carriage: duplicate copies, group/ungroup and
  geometry carry, a merge keeps the LOWEST entry's label (the `doc_identity` rule), flatten is unlabelled.
- **`.rz` carriage** — conditional outermost wrapper **41 (byte) / 42 (typed)** with one `LLBL` table
  (`u32 schema=1`, `u32 count≥1`, ascending `(u32 index, u32 code 1…7)` pairs; byte index bottom-first as
  `MOUT`, typed index = postorder record index as `LCLP`). Written only when some entry is labelled, so an
  unlabelled document's bytes are exactly the prior writer's; malformed tables refuse before any PNG decodes.
  Later wrappers nest outside it. PSD carries `lclr` both ways at both depths; a malformed `lclr` body or
  code > 7 drops to 0 rather than refusing the file (a swatch is cosmetic).
- **The panel filter** (name/kind/label) changes no document. While active it walks
  `tree.visibleRows(collapsedHidden: false)` so a match inside a CLOSED group gets its own row with its own
  indent, and skips `mappedToVisibleRows` so clicking selects that entry. The panel and `find_layers` share
  `LayerFilter.matches`. No MCP twin for the filter bar (view-preference exemption, `app/CLAUDE.md`).
- **Shared preference-library contract** — `../app/Sources/PreferenceLibrary.swift`
  (`PreferenceLibraryCodec` + `PreferenceLibrary<Codec>`): bounded reads, digest file names, advisory `flock`,
  atomic publish, `{entries, broken}` listings, delete by exactly one of `name`/`file`, and refusal (not
  overwrite) of an occupied canonical slot. Every sentence is built from the codec's `noun`. Codecs and
  bounds: RAW 128×8 KiB (schemas 1…2), live filter 128×8 KiB, export 64×4 KiB (1…2), metadata policy
  64×4 KiB, document presets 64×4 KiB (`RZ_DOCUMENT_PRESETS_DIR`), shortcuts 4×32 KiB (`RZ_SHORTCUTS_DIR`),
  workspaces 32×4 KiB (`RZ_WORKSPACES_DIR`). **Repair stays RAW-only**; `RawPresetLibrary` keeps its
  `Fault`/`Broken`/`Entry`/`Listing`/`repair(file:)` API as wrappers.
- **Recordability:** `set_layer_label` is a step (with the `layers` set form; reply lists written `layers`,
  unchanged `skipped`, and `changed`; all-skipped pushes nothing). `new_document` and `find_layers` are
  `nonRecordable` (`new_document` deliberately NOT `notAStepTool`, like `open_document`). Library tools are
  `nonRecordable` + `notAStepTool`. All 12C tools are in `DocumentPrecision.commonTools` and `FloatDocument.tools`, except the library tools that read no
  document (`list_/save_/delete_document_preset(s)`, `list_workspaces`, `delete_workspace`), which are in
  `documentFreeTools`, and `new_document`, which reads no document either and is in `documentFreeTools`.
- **Navigator** (View ▸ Navigator, ⌃⌘N) — a right-panel TAB (index 4), not a floating panel
  (`NavigatorPanelViewController.swift`, `EditorViewController+Navigator.swift` with `EditorViewState`).
  Reads `ImageCanvasView.image`, never assigns it (the setter clears overlay/selection/Quick Mask). Thumbnail
  cached by CGImage identity and rebuilt only when visible and not live. MCP `get_view` / `set_view`
  (`AgentServer+Workspace.swift`; 12D's view arguments in `AgentServer+Views.swift`) are `nonRecordable`; zoom outside
  0.02…32 and an off-canvas centre are refused by name, not clamped; replies are read-back state.
- **Draggable right panel**: `DS.panelWidthRange` 240…560, `ViewToolOptions.panelWidth` (default 304),
  `PanelDividerView` (7 pt grab band, 1 px line at x=3, added last to hit-test first). One
  `panelWidthConstraint` drives all tabs; `applyViewChrome()` is the single change path.
  **No panel subview may pin a width to `DS.panelWidth`.**
- **Customizable shortcuts** (Settings…, ⌘,) — `Shortcuts.swift` (`ShortcutChord`, `ShortcutBindings`,
  `ShortcutCodec`, `ShortcutMenu`), `ShortcutsSheetController.swift`, `AppDelegate+Shortcuts.swift`.
  Identity is the SELECTOR (+ `#tag` when non-zero), never the title. Defaults are a launch-time SNAPSHOT of
  `buildMainMenu()`; the file stores only overrides (one library item named `"Shortcuts"`), applied to the
  live `NSMenuItem`s so the command palette needs no change. A chord must contain ⌘ or be F1–F19 (menu
  equivalents resolve ahead of the first responder). Conflicts are checked against the whole walk (exact chord
  comparison — see [phase-12d-comparison-views-status.md](phase-12d-comparison-views-status.md) for why exact is right)
  and refused naming the holder. Not customizable: app menu, `ShortcutMenu.systemSelectors`, Open Recent,
  dynamic lists (`representedObject`), hidden/alternate items. A broken file applies NOTHING.
- **Saved workspace layouts** (Window ▸ Workspaces) — `Workspace.swift` (`WorkspaceLayout` schema 1),
  `EditorViewController+Workspace.swift`, `AppDelegate+Workspaces.swift` (menu rebuilt in `menuNeedsUpdate`).
  Stores window frame, panel visibility/tab/width, rulers/grid/snap and current tool — **never zoom or
  scroll** (per document, `EditorViewState`). Tabs restore through the existing entry points
  (`showInfoTab()`, `showNavigatorTab()`), never a bare `panelTab = …`. Frame goes through
  `WindowFrameFit`: a frame that lies inside the usable area (the union of the screens' visible frames)
  is kept exactly. Otherwise it is moved, and shrunk if needed, wholly onto one display. A full-screen window
  keeps its frame; a tool the depth refuses is skipped silently. UI save uses `replace: true`, MCP
  `save_workspace` defaults `replace: false`. Workspace tools are `nonRecordable` + `notAStepTool`.
- **No MCP twin for shortcuts** — app-wide preference with no honest `document_id`, undo step or saved carrier.
- **Broken library files in the UI** — `PreferenceLibraryBrokenSection.swift`
  (`BrokenLibraryFilesSection`, lifted out of the RAW manager) lists each bad file as "file — reason" with
  Delete File, and Repair on RAW only. The RAW, filter, export and metadata-policy managers and the New Document
  card use it; each reads `listing()` once. Window ▸ Workspaces ▸ Delete Workspace adds a "Delete Broken File"
  group (rows carry the file; `deleteWorkspaceFile:` is depth-independent), and a library-level fault replaces
  "No Saved Workspaces" with its sentence. Delete File is **unconfirmed**, for RAW parity: it removes a file no
  build can read, and MCP's `delete_* {file}` twin asks nothing either.

## Rejected alternatives
- Rebinding ⌘N to New Document — renegotiates a shortcut already in users' fingers.
- Navigator as a floating panel — needs its own controller, frame persistence and close story; a second VIEW is 12D's job.
- Keying shortcuts by menu title — orphaned on rename/localisation.
- Storing zoom in a workspace — would jump every document to a magnification chosen for another.
- Shortcuts as a bare JSON file — a stray copy would refuse the folder instead of being a diagnosed `broken` row.

## Consequences
- Any new preference library must be a `PreferenceLibraryCodec`, not a bespoke reader.
- Any new menu item is automatically rebindable unless it is in `systemSelectors`, dynamic or hidden; its
  default comes from the build's menu, so changing a default in code changes it for everyone without overrides.
- The 9H UI probe's tablet-drift compensation (`send()` iterates the affine round trip to exactly zero) is
  load-bearing for `airbrush-ui`'s exact dab comparison.

## Open items (verified 2026-09-26)
- `DONE` — broken-file rows in the live-filter, export and metadata-policy
  sheets and the New Document card (the section above), gated by the 9H case `broken-library-managers-ui`
  (with `new-document-broken-presets-ui` and `workspace-broken-files-ui`). The shortcuts sheet still stops at a
  message line.
- `DONE` — a broken file on a name's canonical slot is
  no longer a dead end in the UI: the export manager lists it and Delete File clears the slot, so the save
  succeeds (`broken-library-managers-ui`); Workspaces gets the same through Delete Broken File.
- `OPEN` — a repair path for non-RAW libraries (needs per-library field recovery; `PreferenceLibrary.swift`
  reports `repairable: false`). Unowned.
- `OPEN` — bare canvas tool-key remapping, per-document / named shortcut sets, Photoshop `.kys` import. Unowned;
  the shortcut file can take a second item without a schema bump. The bare tool letters now have defaults (a
  shared key cycles in rail order); remapping, named sets and `.kys` import remain open.
- `DONE` — frame recovery: `WindowFrameFit` in `applyWorkspace` (see above), gated by the
  9H case `workspace-frame-fit-ui` and the 12C-2 drive's W2.
- `OPEN` — workspace auto-restore per document; awaiting a maintainer decision. Unowned.
- `DONE` — second synchronized view and layer comps (12D, `5adcb8d`, `8c99934`).
- `DONE` — `lclr` on carried adjustment records at import: `../core/src/psd.rs` (`label: records.layer_labels[i]`)
  and `adjustment_layer(..., label)` in `../core/src/psd_import16.rs`. No test fixture exercises it;
  uncarried adjustment/fill records are omitted whole, label included.
- `DONE` — 9H probe tablet drift fixed; carried nits (library-full wording, `NewDocumentProfile.data() throws`).
- `DROPPED` — 32-bit float and Gray New Document: refused by design (`../app/Sources/NewDocument.swift` header);
  no promotion into float exists anywhere (also refused by 12G-2's clipboard arms) and the core is RGB-only.
