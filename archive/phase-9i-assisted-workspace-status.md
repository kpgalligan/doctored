# Phase 9I assisted annotation workspace and replay pipeline — status

**To run a session, follow the [assisted annotation runbook](phase-9i-assisted-annotation-runbook.md).** The session is deferred at the owner's discretion (2026-09-16).

**What this is.** The collection half of the
[assisted edge refinement protocol v1](phase-9i-assisted-protocol-v1.md): a
source-only workspace exporter and a blinded, budgeted annotation window that
writes one protocol-valid replay per case. It is a **measurement instrument**.
It enables no editing control, exposes no matting capability, and produces no
score. 9I and P7-12 remain open; [gate v2](phase-9i-gate-v2-status.md) is
untouched, and no human annotation has been collected.

**The scorer is phase 2**, and it now exists:
`core/examples/phase9i_assisted_quality.rs` plus
`scripts/phase9i_assisted_scoring.py`, described in
[the assisted scoring note](phase-9i-assisted-scoring-status.md). Nothing in
this phase anticipated its result, and no assisted result exists yet: only a
synthetic dry run has been executed.

## The exporter

`scripts/phase9i_assisted_workspace.py OUTPUT --executable P --crops D --full D
--annotator-id ID [--exposed CASE …] [--cases CASE …]`

1. `checked_output` refuses an OUTPUT that exists, is a symlink, or is inside
   the repository — the `phase6_reference_fixtures.checked_root` precedent.
2. Both pinned fixture directories are re-verified first, with the same
   `--verify-only` commands `phase9i_soft_prior_qualification.py` runs.
3. The population is `phase9i_gate_v2_rescore.ASSISTED_COMPLETION_SUBSET` (36
   cases). `--cases` restricts it and records `subset_override: true`;
   `--exposed` entries must be in the subset.
4. A fresh `secrets.token_hex(16)` salt gives each case a 12-hex anonymous id
   (`sha256(f"{salt}:{case}")[:12]`), collision-checked.
5. `core/examples/phase9i_assisted_workspace.rs` rebuilds each case through the
   FROZEN evaluator recipe — `frozen::photo_source(&fg, background)` for the
   composite and `frozen::prior(&truth, start)` for the starting mask, with the
   photographic corpus taking the pinned `[512, 384, 1280, 1280]` crop of a
   2304x3456 reference and the full corpus the whole declared image — and
   writes `source.png` (RGB8) and `prior.png` (Gray8). Truth is read to derive
   those two planes and is never written, named or hashed.
6. Every plane is decoded independently (`verify_phase2.read_png`) and hashed
   into `case.json` and `manifest.json`.

Layout:

```
OUTPUT/workspace/{manifest.json, TASK-CARD.md, cases/<anon>/{source.png,prior.png,case.json}, replays/}
OUTPUT/private/{mapping.json, jobs.json, export.log}
```

`mapping.json` — the only file that knows which anonymous id is which case —
holds the salt, the repo head, the declared exposure list, and
`program_sha256` over this script, the exporter binary, the template's six
`recipe_source_pins` recipe sources and `phase9i_gate_v2_rescore.py`, beside
`protocol_sha256`, `task_card_sha256` and the two reference-manifest hashes.

**The audit raises rather than warns.** `audit_workspace` refuses a workspace
whose file or directory set is not exactly the layout above, whose source PNG
is not colour 2 depth 8 or prior PNG not colour 0 depth 8, which carries any
PNG chunk beyond IHDR/IDAT/IEND, or in which any path or any JSON string —
key or value — contains a case id, a reference id or the tokens `photo-`,
`full-`, `truth`, `alpha`, `reference`.

## The replay format

`core/src/assisted_replay.rs`; its module doc **is** the schema table, and
`core/include/rasterize_core.h` states the same contract for the two exports.
The core is the replay's only reader and validates a whole document, in
progress or final. It never hashes, never decodes a PNG and never scores; the
label plane is an all-128 base with the reconstructed ACTIVE strokes applied in
order through the unchanged `rz_trimap_strokes` rasterizer, so no capsule math
is duplicated and that constructor's 64-million-visit cap applies unchanged.
A refusal from it is reported with a `rasterizer_limit: ` prefix, which is what
lets the app record a counted rejection instead of treating it as a bug.

The session rules — when the clock starts, what a gesture costs, what a
rejection means, and why the write planes are constants — are pinned in the
protocol's [Tooling pins](phase-9i-assisted-protocol-v1.md#tooling-pins-2026-09-15)
section.

## The window

Behind `RZ_ASSISTED_ANNOTATION` (or the `AssistedAnnotationEnabled` default).
Off in every ordinary launch: the File item does not exist, and all five MCP
handlers refuse with `AssistedAnnotation.disabledMessage`.

No `ImageDocument` is involved — the window owns its own `RasterImage`, there
is no `applyEdit`, no `registerUndo` and no dirty state. Undo and Redo are
recorded gestures replayed through the core, not history, and the canvas view
implements `undo:`/`redo:` itself because `NSWindow` precedes its window
controller in the responder chain. Quit is intercepted through the nil-target
`terminate:` when the annotation window is key, and through the existing
`applicationWillTerminate` otherwise; there is deliberately **no**
`applicationShouldTerminate(_:)`, which in this NSDocument-based app would
shadow `NSDocumentController`'s unsaved-document review.

Blinding is enforced rather than documented: a human session refuses every MCP
stroke and MCP finalize, and a synthetic session records `annotator_id: null`,
`origin: "synthetic"` and `scoreable: false` and never presents the task card.

## Changed recipe hash

`core/examples/phase6_matting_quality.rs` changed by one word — `fn prior`
became `pub(crate) fn prior` so the workspace exporter can reuse the frozen
starting-mask recipe instead of copying it. That changes its sha256, which
[the declaration template](../designs/phase-9i-assisted-v1.template.json)
`recipe_source_pins` and older runs' `predeclared.json` pin. As gate v2 already
established, a future run pins the new hash; the template is not edited and no
preserved run is touched.

- old: `14fc47e3b884cb22618cb8182bb25fb7f2ee6f2e2d52f7f1e85ba050f664d63e`
- new: `52822167684dad69a3f26de98869d3c2ff5d2c11926e7b11ea103c7256f14351`

## Verification

- `core/tests/assisted_replay.rs` — black-box FFI only. NULL tolerance for both
  exports; a two-stroke plane equal to an INDEPENDENT capsule oracle (endpoint
  half planes plus cross-product distance, never the production projected
  point); undo/redo stack reconstruction including a cleared redo stack and an
  empty-stack refusal; `rejected:rasterizer_limit` and `rejected:time_budget`
  strokes each costing a gesture without being applied; sixteen invalid
  documents refusing with the output buffer untouched, including a >4 MiB
  payload; four full-canvas capsules on 4096x4096 refusing the plane with the
  `rasterizer_limit: ` prefix while the summary still succeeds; and
  `final: false` on an in-progress document.
- `scripts/test_phase9i_assisted_workspace.py` — exporter naming, output
  refusals, PNG header reading and the unblinding audit, on fabricated bytes.
- `scripts/verify_phase9i_assisted.py` — an owned production MCP drive of the
  built app against a real exported workspace: the pending case list, a
  synthetic open, the zero-gesture state, three accepted short strokes (one per
  label, with the Unknown stroke retracing the Foreground one because Unknown
  RELEASES a constraint and would otherwise change nothing), then
  corner-to-corner diameter-81 strokes until the rasterizer's cumulative visit
  cap records a `rejected:rasterizer_limit` gesture that leaves the plane hash
  unchanged, the written replay's provenance and per-event hashes, the
  one-attempt lock, and a second instance launched WITHOUT the flag refusing
  with the disabled message.

**Drive receipt (2026-09-16).** Workspace
`~/devel/rasterize-fixtures/phase9i-reports/assisted-workspace-drive`, exported
with `--cases photo-0051115Q_000001_0003-light-blurred --cases
full-portrait-0051115Q_000001_0003-checker-binary`. The synthetic session ran
on the 2304x3456 full case: 12 modifying gestures and 14 events, the ninth
corner-to-corner stroke recorded `rejected:rasterizer_limit` (each such capsule
costs the whole 7,962,624-pixel canvas against the 64,000,000 cap), outcome
`submitted`, `scoreable: false`, and re-opening the case refused as locked.

A 763x764 or 1280x1280 case cannot reach that cap inside twenty gestures, which
is why the drive exports a full-size portrait case rather than the smallest
available one; the exporter itself is indifferent to which subset cases are
named.
