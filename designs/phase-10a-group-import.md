# Bounded PSD folder-property import (ADR)

- **Status:** Accepted — closed (superseded as the 10A status record by [phase-10a-qualification.md](phase-10a-qualification.md))
- **Phase / dates:** 10A folder slice, 2026-09 (before the 2026-09-13 10A closeout)
- **Original:** [archive/phase-10a-group-import.md](../archive/phase-10a-group-import.md) — full history, measurements, receipts
- **Key commits:** `275f193`

## Context
Layered PSD import used the third-party `psd` 0.3.5 crate, which builds each `PsdGroup` from the
*closing divider* and discards the optional `lsct` blend key. Folders therefore came in as visible, fully
opaque Pass Through groups regardless of what the file said. The fix had to retain folder opacity,
visibility, blend mode and open/closed state without writing a second PSD parser or compositor, and had
to bound the metadata walk so hostile files cannot drive the legacy recursive emitter.

## Decisions
- `core/src/psd_group_records.rs` is the **one bounded layer-record walk**, run by both preflight
  (`dr_psd_import_report`) and layered open (`dr_doc_open_psd`) *before* the third-party parser. It frames
  and interprets properties only; it never decompresses pixels. *Why:* one framing implementation for the
  report and the open, verified before untrusted data reaches the dependency.
- Folder properties come from the **folder record**, never the closing divider. The `lsct` blend key wins
  when present, else the record's own key — so Normal folders stay isolated and Pass Through stays Pass
  Through. Visibility: record flag bit 1 set = hidden. *Why:* Adobe spec; the dependency gets this wrong.
- Correlation with the dependency's tree is by parent/ordinal sequence, never by name (duplicate names
  are legal).
- Recognized keys map through the existing blend conversion (`PSD_BLEND_KEYS`) and the existing group
  compositor; an **unknown folder blend key refuses layered import**.
- Refuse on incoherent framing: duplicate/unknown divider records, unsupported `lsdk`, malformed Unicode
  names. The `luni` count must stay inside its own block (the dependency otherwise reads ahead and can
  manufacture names from later bytes).
- Bounds (validated before decoder entry): 2,048 raw records (`MAX_RECORDS` = 2 × native row cap),
  1,024 imported rows, 56 channels/record, 64 MiB aggregate extra data (`MAX_EXTRA_BYTES`), 65,536
  additional blocks, 128 nested folder records (`MAX_RECORD_DEPTH`). Additional blocks use even declared
  lengths.
- The existing ten-level editing limit dissolves deeper folders with a reported loss; nesting above 128
  refuses before the recursive emitter.
- A layered refusal never silently substitutes the saved composite; the composite stays a separate,
  explicit choice with its own limits. Both choices keep supported RGB ICC/PPI.
- Oracles in `core/tests/psd_group_import.rs` are independent source-over/multiply arithmetic on
  hand-built PSD bytes, not importer output.

## Rejected alternatives
- Forking/patching the `psd` crate — avoided; retain properties in our own pre-walk instead.
- Matching folders to groups by name — breaks on duplicate names.
- Using closing-divider properties as a fallback — they are not the folder's properties.

## Consequences
- Every later PSD slice (pixels, masks, relative masks, 16-bit, constructs, feature markers) hangs off
  this walk; new record-level parsing belongs here, not in a parallel reader.
- The bounds above are part of the admission contract; relaxing them changes refusal behaviour.

## Open items (verified 2026-09-26)
- `DONE` — Mask import (then only reported as losses): [phase-10a-masks-status.md](phase-10a-masks-status.md), `b802115`.
- `DONE` — Off-canvas / complete raster storage: [phase-10a-pixels-status.md](phase-10a-pixels-status.md), `2a7d042`.
- `DONE` — Native-depth PSD: [phase-10c-depth-status.md](phase-10c-depth-status.md), `a3d29b7`.
- `DONE` — Independent-application reopen: GIMP corpus in [phase-10a-qualification.md](phase-10a-qualification.md) (`352cb50`); Photoshop runs 2–4 (plan §10C).
- `DONE` — Group clipping / Blend If on import: 10C constructs slice and 10D (carryover P7-20 verified).
- `DONE` — Layer locks: declared a non-goal of the closed 10A contract, then read and written at both depths (`psd_locks.rs`; [phase-10a-qualification.md](phase-10a-qualification.md)).
