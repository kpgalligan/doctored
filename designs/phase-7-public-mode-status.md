# Public 16-bit document mode (ADR)

- **Status:** Accepted — open items remain ([#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 7E, 2026-09-10 (downstream notes through Phase 12F)
- **Original:** [archive/phase-7-public-mode-status.md](../archive/phase-7-public-mode-status.md) — full history, measurements, receipts
- **Key commits:** `3bd4349` (follows 7D `7e60190`)

## Context
7B–7D built a qualified typed workflow behind internal owners. 7E exposed it as a public editor mode: the typed
document had to live inside the existing document, editing-state, Undo, save, snapshot and recovery machinery without
a second Undo system, without byte surrogates, and with every unsupported tool explicitly refused. Current
per-operation support is the [operation matrix](phase-7-operation-support.md); remaining gaps were rows in the
[carryover ledger](phase-7-carryovers.md), closed 2026-10-30, whose open rows are issues.

## Decisions
- **One owner, either depth.** `RasterDocument` (`app/Sources/DoctoredCore.swift`) owns exactly one byte or typed core
  document; the central editing state, Undo/history, saved identity, snapshots and recovery keep that owner. Typed
  owners never contain a byte surrogate; the authoritative u16 projection is stored and addressed separately from
  display/interchange conversions.
- **Opening:** preserving PNG/TIFF16 and typed native open on the existing worker; native signatures beat filename
  extensions; unsupported preserving inputs fail without the byte ImageIO fallback or working-space adoption.
- **Conversion is explicit, undoable and flattening** (Image ▸ Mode, MCP `get_precision` / `convert_precision`,
  `app/Sources/PrecisionModeConversion.swift`): byte→16 evaluates the legacy composite then ×257; 16→8 evaluates the
  typed composite then applies the chosen quantization (none / ordered RGB dither, alpha never dithered). Before/after
  preview; discarded plane targets reset inside the same Undo transition; same-precision requests refuse; Actions keep
  the depth choice and exact authored colors. Numerical contract: [conversion and readout](phase-7-conversion-readout.md).
- **One capability gate** for UI, MCP and Actions — `DocumentPrecision` (`app/Sources/DocumentPrecision.swift`) mapped
  onto the core family table (`core/src/precision_capability.rs`), plus core parameter/resource checks. Legacy byte
  APIs never receive a typed pointer. AppKit supplies only scalar drawing coverage; color and source pixels stay typed.
- **Readouts** use authoritative u16 pixels; 256 displayed histogram bins never stand in for exact clipping counts.
- **Save/export:** native master save preserves editable typed state; captured export offers PNG/TIFF16 or deliberate
  8-bit with a loss report and does not replace or mark a newer master saved. PNG/TIFF16 reuse the **captured current
  u16 projection** (`core/src/precision_projection_export.rs`) — no second full-frame render.
- **Editing-state archive v2** (now v4, `app/Sources/EditingStateArchive.swift`) carries storage/evaluation/export
  policy with v1 byte compatibility; recovery includes named snapshots; decode peaks count toward admission.
- **Staleness and reentrancy:** typed preparation finishes before mutation/history publication; worker results bind
  document identity, revision, target and selection, and stale work refuses; history publication excludes reentrant
  mutations and nested Undo/Redo.
- **Fixes established by qualification (still binding):** `withUndoGroup` (`app/Sources/UndoGrouping.swift`) disables
  automatic grouping only when opening a group at level zero, closes its own group and restores policy — modal-timer
  edits had left AppKit expecting a drained implicit group at Save. Actual window close clears and redraws cached
  display images before AppKit can retain closed views (they otherwise kept the CGImage provider and its pixel buffer
  alive; `EditorViewController+DisplayLifecycle.swift`).
- **Outcome:** 999 Rust tests; `make typecheck` 0 errors and `make app`; 25 built-app MCP groups (16 legacy, 9 typed);
  18 Swift + 2 codec groups; a public-window 24 MP run (open, typed paint, Undo/Redo, native Save/reopen, PNG16
  export/reopen, close) passed 9/9 stages in 6.94 s, 2.25 GiB RSS, export admission 1.96 GB < 2 GiB, all tracked
  resources zero after close. Reproduce: `python3 -B scripts/precision-probe/verify.py --keep --large-public`.

## Rejected alternatives
- A second typed editor/Undo stack — would fork history, save and recovery semantics.
- Rendering a second projection for 16-bit export — exceeded the public retained scope at 24 MP.
- Treating the display CGImage or byte clipboard as a typed master.

## Consequences
- Every new typed tool must be admitted through `DocumentPrecision` + `precision_capability.rs` and refused by name
  otherwise; the matrix must stay consistent with that gate.
- Policy at 7E was normalized encoded SDR; linear light (11E) and float (11F) are separately versioned modes.
- The 24 MP result qualifies a single-edit workflow, not arbitrary multilayer/history sizes or every export format.

## Open items (verified 2026-09-26)
- `DONE` — every capability listed as unavailable at 7E close: drag transforms/canvas Move (12F-1c, `f285a29`),
  pressure/airbrush (P7-13), selected destructive tone and selection-gated histograms (P7-06/P7-07), color-match fill
  (P7-14), channel↔selection at u16 (P7-11), scalar-plane painting (P7-08), clipboard (P7-29 closed, 12G), styles and
  other blends (P7-19/P7-20), content-aware (P7-18), retained text/shapes (P7-04), 16-bit PSD (P7-21), typed RAW
  packets/PPI (11D), camera auxiliary mattes (9G), invalid-preset repair (12B).
- `DONE` — edge brushing and colour decontamination ship in Select and Mask (2026-10-11, carryover P7-12); the Phase 6
  cutout quality gate is no longer a ship condition and its assisted session is dropped
  ([gate v2](phase-9i-gate-v2-status.md)).
- `DONE` — linear-light phases 2–3 (carryover P7-24, 2026-11-12).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — live codec cancellation, compression of
  native/snapshot payloads (carryover P7-32).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — larger multilayer/history/export workloads
  (carryover P7-33).
