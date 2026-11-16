# Named and per-document shortcut sets, and `.kys` import (ADR)

- **Status:** Proposed — decided on paper, nothing built; open under
  [#20](https://github.com/kpgalligan/doctored/issues/20).
- **Builds on:** [phase-12c-workspace-status.md](phase-12c-workspace-status.md) (the `"Shortcuts"` library item,
  its menu overrides and, since the remappable tool keys, its `tool_keys` map).

## Context

One shortcut set exists: the `"Shortcuts"` item of the shortcut `PreferenceLibrary` (`DR_SHORTCUTS_DIR`, 4 files
× 32 KiB), holding the menu overrides (selector identity → chord or null) and the bare tool keys (`tool_keys`,
tool identifier → key or null). Defaults are never stored: they are the build's menu snapshot and
`EditorTool.defaultKeyCharacter`. The open asks are several named sets to switch between, a set chosen per
document, and reading a `.kys` file — the shortcut file Photoshop exports — as a compatibility import.

## Decisions (proposed)

- **A named set is one more library item.** `ShortcutBindings` gains a user-chosen `name`; the codec keeps one
  item per set, the menu overrides and `tool_keys` together (which is why the tool keys went into the same item
  rather than a second one). `"Shortcuts"` stays the default set's name, so every existing file is the default
  set unchanged. The library's cap rises from 4 files to 16 (a set is far under 32 KiB).
- **The active set is an app-wide preference** (`UserDefaults`, the set's name), like the view preferences: no
  undo step, no `document_id`, no MCP twin — the shortcut exemption in `app/CLAUDE.md` applies unchanged.
  Switching applies the set through the existing full `ShortcutMenu.apply` and `ToolKeys.load`; a set that
  fails to read applies nothing and the default stays, as a broken file does today.
- **Editing writes the active set.** The Shortcuts tab gains a set popup with New…, Duplicate, Rename and
  Delete; the default set can be reset but not deleted.
- **Per-document sets: not a document property.** A keyboard layout is view state with no undo or save value —
  the reason 12D dropped persisted view rotation — so no DRDC word carries one. Recommended: drop per-document
  sets and keep named sets only; the fallback, if the owner wants them, is a remembered set name per file path
  in app preferences, applied when that document's window becomes key. Held for the owner (below).
- **`.kys` import maps onto identities and tool identifiers.** The importer reads a `.kys` file's menu
  commands and tool keys, maps each onto a selector identity (`ShortcutMenu.identity`) or an `EditorTool`
  identifier through an explicit table, writes the result as a NEW named set (never over the active one), and
  reports every entry it could not map by name. Chords still pass `ShortcutChord.validated()` and the
  conflict walk; tool keys pass `ToolKeys.validated`. Nothing is guessed from a title.

## Held for the owner

- **The `.kys` format is unread.** No reference `.kys` file is in this repository or its fixtures, and getting
  one is a new preset export from Photoshop, which the issue loop may not make. Options: the owner drops one
  `.kys` export into `~/devel/doctored-fixtures`, or the import is dropped. Recommendation: supply one file;
  the mapping table and the importer are then a small batch. A `.kys` file is read for its format only and is
  never committed.
- **Per-document sets.** Options: (1) a remembered set name per file path in app preferences; (2) a set name
  stored in the `.doctored` file under a new DRDC word; (3) drop them and keep named sets only.
  Recommendation: (3), or (1) if the need is real.

## Consequences

- The library's cap and the sheet change; the stored item's shape does not (a set without a name field reads as
  the default set), so a file written today reads in every later build.
- An older build that meets a file with `tool_keys` or a set name reports it as a broken row and applies its
  defaults (downgrade only), as it already does for `tool_keys`.
