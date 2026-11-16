# Named and per-document shortcut sets, and `.kys` import (ADR)

- **Status:** Accepted — named sets and `.kys` import shipped; per-document sets held under
  [#20](https://github.com/kpgalligan/doctored/issues/20).
- **Builds on:** [phase-12c-workspace-status.md](phase-12c-workspace-status.md) (the shortcut library item,
  its menu overrides and, since the remappable tool keys, its `tool_keys` map).

## Context

Before this batch one shortcut set existed: the `"Shortcuts"` item of the shortcut `PreferenceLibrary`
(`DR_SHORTCUTS_DIR`, 4 files × 32 KiB), holding the menu overrides (selector identity → chord or null) and the
bare tool keys (`tool_keys`, tool identifier → key or null). Defaults are never stored: they are the build's
menu snapshot and `EditorTool.defaultKeyCharacter`. The asks were several named sets to switch between, a set
chosen per document, and reading a `.kys` file, the keyboard-shortcut exchange format, as a compatibility
import.

## Decisions (as built)

- **A named set is one more library item.** `ShortcutBindings` stores its `name` and the decoder admits any
  non-empty name (the library's normalized-name check still applies); one item per set holds the menu
  overrides and `tool_keys` together. `"Shortcuts"` stays the default set's name, so every existing file is
  the default set unchanged. The library's cap rose from 4 files to 16 (a set is far under 32 KiB).
  `ShortcutSets.swift` holds the operations.
- **The active set is the library's `.default` file** (`PreferenceLibraryDefault<ShortcutCodec>`), not
  `UserDefaults` as first proposed: every probe and owned instance already isolates `DR_SHORTCUTS_DIR`, a
  delete clears a pointer naming the deleted set, and Export's and the proof preset's defaults use the same
  shape. The default set is active exactly when `.default` is absent or names no listed set, so deleting the
  active set (or its file) falls back to it. App-wide, no undo step, no `document_id`, no MCP twin — the
  shortcut exemption in `app/CLAUDE.md` applies unchanged.
- **Switching** loads the set (the default set with no file on disk is empty bindings), applies it through
  `ToolKeys.load` and the full `ShortcutMenu.apply` FIRST, then writes the pointer, whose change notification
  the rail's tooltips read; a failed write puts the previous set back.
- **Editing writes the active set** (`ShortcutMenu.stored()` reads it; Reset All keeps the set's name). The
  Shortcuts tab's set popup, beside the search field, lists the sets and offers New Set…, Duplicate Set…,
  Rename Set…, Delete Set and Import Shortcuts File…. The default set can be reset but not renamed or
  deleted, and no other set may take its name. A rename at the 16-file bound deletes the old file first and
  puts it back if the new name cannot be saved.
- **`.kys` import writes a NEW set** named after the file (" 2", " 3"… while taken), made active; nothing is
  written over an existing set and a file of which nothing maps is refused. `KysImport.swift` reads version 4
  (`<photoshop-keyboard-shortcuts>`: `<command kind="static" id=…>` and `kind="dynamic" name=…` with
  `<shortcut>Opt+Shift+Cmd+K</shortcut>` children, `<tool type="1" key=…>` letters, `<taskspace>` blocks) and
  maps through explicit tables only: static commands by `id` (their names are localized), dynamic ones by
  name, tools by `key`. A command's first shortcut is its chord (a further one is refused), no `<shortcut>`
  removes the binding, an empty tool letter removes the key, `modified` is ignored. Chords pass
  `ShortcutChord.validated()` and `assign`'s conflict rule (the fixed chords, an earlier entry), and a chord
  equal to the default of a command the file leaves alone is refused rather than silently stripping that
  command (`apply` withholds such a default), repeated to a fixed point so a swap within one file stays. Tool
  keys pass `ToolKeys.validated`. The report lists applied, not mapped and refused entries by name. The format
  was read from an installed reference file; nothing of it was copied, and the tests use hand-written text.
- **Per-document sets: not a document property.** A keyboard layout is view state with no undo or save value —
  the reason 12D dropped persisted view rotation — so no DRDC word carries one. Held for the owner (below).

## Held for the owner

- **Per-document sets.** Options: (1) a remembered set name per file path in app preferences, applied when
  that document's window becomes key; (2) a set name stored in the `.doctored` file under a new DRDC word;
  (3) drop them and keep named sets only. Recommendation: (3), or (1) if the need is real. Not built.

## Verification

jobs-probe "shortcut sets" and "kys import" (`scripts/jobs-probe/README.md`), each seen red under its
mutants. Owed by hand (they take the Mac's focus): the set popup's switching and its New, Duplicate, Rename
and Delete prompts; Import… with the open panel and the report alert; a relaunch keeping the active set;
`scripts/verify_phase9h_ui.py`'s `shortcut-edit-ui` and `shortcut-relaunch-ui` on the new tab layout.

## Consequences

- An existing `"Shortcuts"` file reads unchanged as the default set; the stored item's shape is unchanged
  apart from the name it carries.
- An older build that meets a named set lists it as a broken row and applies its defaults, and with more than
  4 files it refuses the whole listing (downgrade only), as it already does for `tool_keys`.
- The Shortcuts tab's table is 290 pt tall (was 320) so the Settings card, set by its tallest tab, stays within
  700 pt (jobs-probe "settings card height").
