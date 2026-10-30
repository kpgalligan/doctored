# Phase 9I assisted annotation workspace and replay pipeline (ADR)

- **Status:** Accepted — closed (the instrument is built; the session it serves is tracked elsewhere)
- **Phase / dates:** 9I assisted protocol phase 1, 2026-09-16
- **Original:** [archive/phase-9i-assisted-workspace-status.md](../archive/phase-9i-assisted-workspace-status.md) — drive receipt, hash history
- **Key commits:** `de70228` (workspace + window), `5f75604` (scorer follow-up), `81d6aa5` (runbook link)

## Context
The collection half of [assisted protocol v1](phase-9i-assisted-protocol-v1.md): export a source-only, anonymous
workspace, and let a blinded human paint budgeted label strokes that are recorded as a protocol-valid replay per
case. It is a **measurement instrument** — no editing control, matting capability or score reaches the app. To run
a session, follow [phase-9i-assisted-annotation-runbook.md](phase-9i-assisted-annotation-runbook.md).

## Decisions
- **Exporter:** [`scripts/phase9i_assisted_workspace.py`](../scripts/phase9i_assisted_workspace.py)
  `OUTPUT --executable P --crops D --full D --annotator-id ID [--exposed CASE …] [--cases CASE …]` with
  [`core/examples/phase9i_assisted_workspace.rs`](../core/examples/phase9i_assisted_workspace.rs).
  - Refuses an OUTPUT that exists, is a symlink or is inside the repository.
  - Re-verifies both pinned fixture directories (`--verify-only`) first.
  - Population is `phase9i_gate_v2_rescore.ASSISTED_COMPLETION_SUBSET` (36); `--cases` restricts it and records
    `subset_override: true`; `--exposed` must name subset cases.
  - Anonymous id = `sha256(f"{salt}:{case}")[:12]` with a fresh `secrets.token_hex(16)` salt, collision-checked.
  - Rebuilds each case through the **frozen evaluator recipe** (`frozen::photo_source`, `frozen::prior`; crops use
    `[512, 384, 1280, 1280]` of the 2304×3456 reference) and writes `source.png` (RGB8) and `prior.png` (Gray8).
    Truth is read to derive them and is never written, named or hashed.
  - Copies `designs/phase-9i-assisted-annotation-task-v1.md` verbatim to `TASK-CARD.md` and records its SHA-256.
- **Layout:** `OUTPUT/workspace/{manifest.json, TASK-CARD.md, cases/<anon>/{source.png,prior.png,case.json}, replays/}`
  and `OUTPUT/private/{mapping.json, jobs.json, export.log}`. `mapping.json` is the only file that maps anonymous
  ids to cases; it also holds the salt, repo head, exposure list, `program_sha256` (script, binary, the template's
  `recipe_source_pins`, the re-scorer), `protocol_sha256`, `task_card_sha256` and both reference-manifest hashes.
- **The audit raises, never warns:** `audit_workspace` refuses any file/directory outside the layout, a source PNG
  that is not colour 2 depth 8 or a prior not colour 0 depth 8, any chunk beyond IHDR/IDAT/IEND, and any path or JSON
  string containing a case id, reference id or `photo-`, `full-`, `truth`, `alpha`, `reference`.
- **Replay format:** [`core/src/assisted_replay.rs`](../core/src/assisted_replay.rs) (module doc is the schema;
  `core/include/doctored_core.h` states the contract for `dr_assisted_replay_plane` / `dr_assisted_replay_summary`).
  The core is the only reader, validates whole documents (in progress or final), never hashes, decodes PNGs or
  scores. The label plane reuses `rz_trimap_strokes` so no capsule math is duplicated; its refusals carry a
  `rasterizer_limit: ` prefix so the app records a counted rejection instead of a bug. Session rules are the
  protocol's tooling pins.
- **Window:** gated behind `DR_ASSISTED_ANNOTATION=1` or the `AssistedAnnotationEnabled` default
  ([`app/Sources/AssistedAnnotation.swift`](../app/Sources/AssistedAnnotation.swift)); off in every ordinary
  launch — the File item does not exist and all five MCP handlers refuse with `AssistedAnnotation.disabledMessage`.
  No `ImageDocument`: the window owns its own `RasterImage`, with no `applyEdit`, `registerUndo` or dirty state.
  Undo/Redo are recorded gestures replayed through the core; the canvas implements `undo:`/`redo:` because
  `NSWindow` precedes the controller in the responder chain. Quit is intercepted through nil-target `terminate:`
  and `applicationWillTerminate`; there is deliberately **no** `applicationShouldTerminate(_:)`, which would shadow
  `NSDocumentController`'s unsaved-document review.
- **Blinding is enforced:** a human session refuses MCP strokes and finalize; a synthetic session records
  `annotator_id: null`, `origin: "synthetic"`, `scoreable: false` and never shows the task card.
- `core/examples/phase6_matting_quality.rs`'s `prior` became `pub(crate)` so the exporter reuses the frozen recipe
  rather than copying it; future runs pin the new hash; the template names recipe sources only and carries no hash values.

## Consequences
- Editing the task card file changes what annotators see and the pinned `task_card_sha256`.
- The drive (`scripts/verify_phase9i_assisted.py`) must use a full-size portrait case to reach the rasterizer's
  64M visit cap inside twenty gestures.
- Verification: `core/tests/assisted_replay.rs` (black-box FFI vs an independent capsule oracle, undo/redo,
  rejection accounting, 16 invalid documents, 4096² rasterizer refusal),
  `scripts/test_phase9i_assisted_workspace.py`, and the owned production MCP drive `scripts/verify_phase9i_assisted.py`.

## Open items (verified 2026-09-26)
- `TRACKED` — The real human session over the exported workspace → P7-12 / plan §9I; see the runbook.
- `DONE` — The scorer ("phase 2") exists: `core/examples/phase9i_assisted_quality.rs`, `scripts/phase9i_assisted_scoring.py` (`5f75604`).
