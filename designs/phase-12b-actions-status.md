# Reusable Actions and the RAW preset library (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12B (12B-1 items 1–7, 12B-2 item 8), both shipped 2026-09-18
- **Originals:** [archive/phase-12b-actions-status.md](../archive/phase-12b-actions-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-12b-actions-status.md](../archive/2026-10/phase-12b-actions-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `3e86887` (12B-1), `729524f` (12B-2)

## Context
12A recordings named layers by name and coordinates absolutely, so a rename, reorder or different canvas size
silently re-aimed or broke an Action; assets named by path did not travel; Batch needed a modal window; and a
single malformed RAW preset file refused the whole preset library. Plan text: package assets, stable layer
identities, absolute/relative/anchor geometry, named parameters, batch failure/cancel/collision behaviour, and
per-entry diagnosis/repair/migration for the RAW preset library. No `.rz` change and no `DRDC_VERSION` bump.

## Decisions
- **Stable layer identity** (`../core/src/doc_identity.rs`): `Layer.id` / `PrecisionDocumentLayer.id: u64`
  from ONE process-wide `AtomicU64` for both depths; 0 means "no layer". Rule: *a new picture gets a new id;
  the same picture carried through an op keeps its own* — `Layer::new`, duplicate (per copied entry, whole
  subtree) and a new group container mint; a MERGE keeps the LOWEST merged entry's id; rename, reorder,
  geometry, undo handles carry it. Surface: `dr_doc_layer_id`, `"id"` on typed info rows and on
  `get_document` rows, `RasterDocument.layerRef` / `channelRef` / `layerIndex(id:)`. Tests:
  `../core/tests/layer_identity.rs`.
- **12B made ids session-scoped** (guide/channel precedent) and an Action symbol carries
  `ActionIdentity.session` (UUID per launch, reported as `identity_session`); a stale-session id is never
  tried, the NAME resolves instead. *Superseded for layers by 12E-1/12E-1b* ([phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md)):
  layer ids are persisted (`LIDS`, `../core/src/identity_native.rs`) and honoured cross-session only when the
  record also carries the matching NAME. **Channel ids remain session-only.** A batch over different files is
  the name path by construction.
- **Action schema 2** (`Action.schemaVersion = 2`, `readableSchemaVersions = {1, 2}`); schema 1 decodes
  unchanged. `Action.fileSchemaVersion` WRITES the lowest version that can hold the action, so re-saving an
  untouched 12A recording (Enabled toggle, drag-reorder) produces identical bytes. New symbols:
  `{"$layer": {name,id,session}}` (fallback: topmost layer of that name), `{"$channel": {name,id,session}}`
  (fallback: first channel of that name), `{"$param": name}` (undeclared = decode refusal),
  `{"$asset": file}` (missing names the full path).
- **Geometry modes** (`ActionStep.geometry`: `.absolute` default / `.relative` / `.anchor(CanvasAnchor)` over
  `canvas_size`'s nine anchors; omitted from the file when absolute). Relative: everything scales `v·W'/W`.
  Anchor: `f·W' + (v − f·W)` — distances from the anchor preserved, only positions move. Integer-recorded
  values round to Int. An affine matrix (`transform_mask`, `transform_layers`) is remapped as plain Doubles
  that are never rounded. Anchor keeps the linear part and writes `tx + ox − (a·ox + c·oy)`,
  `ty + oy − (b·ox + d·oy)` with `o = f·(W' − W)`. Relative with no recorded `pivot` conjugates by the remap R,
  `M' = R ∘ M ∘ R⁻¹` (keeps `a`, `d`, writes `b·sy/sx`, `c·sx/sy`, `tx·sx`, `ty·sy`), so the matrix and
  `source_rect` + `corners` forms agree for Double-valued recordings to the corners' four-decimal quantization
  (≤ 0.00005 px; integer-written corners round half away from zero, up to 0.5 px).
- **Pivot rule:** the UI records `pivot: [x, y]` beside a `transform_layers` matrix (⌘T on a set, ⌥⌘T)
  and a `transform_mask` (the sheet's capture-bounds centre); MCP validates it (only with `matrix` on
  `transform_layers`) and never uses it for pixels. Relative with a pivot keeps `a, b, c, d` and writes
  `tx·sx + c·(sx − sy)·py`, `ty·sy + b·(sy − sx)·px` — `transform_layer`'s replay
  `M'(x) = L(x − Rp) + Rp + S·(M(p) − p)`, so angle, scale and skew stay about the remapped pivot; the pivot
  remaps as a position. Uniform scale or no rotation/skew reduces to the conjugation; a recording without a
  pivot replays as before. No Photoshop take; the rule's replay onto a different aspect ratio is unmeasured.
  Drive B10 `pivot-matrix` / `pivot-mask`.
- **`ActionGeometry.fields` is the explicit per-tool table of coordinate ARGUMENTS**; a non-absolute mode on any
  other tool, or on a `native_stroke` dab step, is refused at DECODE by name — remapping the wrong argument is
  silent. A non-absolute step without `recorded_canvas` is refused at decode.
- **The recorded space is COMPUTED, not stored:** `ActionPlayer` starts at `recorded_canvas` and advances by
  `ActionGeometry.canvasAfter` (crop / image_size / canvas_size / rotate 90) only after a step SUCCEEDS.
  Resolution order: `$param` → geometry remap → other symbols → `document_id`.
- **`NSNumber` 0/1 bridges to `Bool`** in Swift; every number guard goes through `ActionJSON.isBoolean` /
  `ActionJSON.number` (CoreFoundation type id). The naive `!(n is Bool)` silently skipped `x = 0`.
- **Named parameters** (`ActionParameter {name, type: number|integer|string|boolean, default?, description?}`)
  are an EDIT, never recorded. `ActionParameters.bind` is the single validator, run by every Play path (Actions
  window, Play Last, `run_action`, `run_batch`, Batch window) before anything runs; `ActionPlayer.run` re-checks.
  A parameter error is an action-level refusal at submission, never per-file batch rows.
- **Asset bundles** (`ActionAssets`): `package_action` writes `<slug>.draction/{action.json, assets/}`,
  rewriting file arguments (`place_image.path`, `add_live_photo_layer.path`, profile paths when
  `profile == "file"`) to `$asset`; basename collisions become `logo-2.png`; a missing source refuses with no
  partial bundle; packaging never modifies the library copy. `import_action` installs assets to
  `<stem>.assets/` keyed on the STORED file stem. Recording stays literal — packaging is the only route to
  `$asset`. `quick_export.destination` is NOT an asset (per-machine output folder).
- **Headless batch** `run_batch` / `batch_status` / `cancel_batch` drive the same `BatchSettings` + Timer-driven
  `BatchRun` with no modal (a modal inside a tool call cannot answer). All Batch rules unchanged. The in-band
  exclusion lives in `ActionPlayer.replayRefusal` behind `isRunningStep`, not in `AgentServer.execute`,
  because the batch's own steps and delivery go through `execute`; `ActionPlayer.dispatchingOwnStep` marks the
  three self-calls; `BatchRun.allowedDuringBatch` = `batch_status`, `cancel_batch`, `job_status`,
  `cancel_job`, `resource_usage`.
- **12A's layer-set gaps closed:** `reorder_layers {layers, at, depth?}` (`at` is the drop line in PRE-move
  numbering; arithmetic shared with the panel in `LayerReorder`), `auto_select_layer.mode: replace|add`,
  `set_layer_lock {layers}`, `set_layer_clipped {layers}` (FOLDS like the menu twin: unchanged entries go in
  `skipped`; refused only when none changed — 12B-2 fix). Single-layer Move now records relative
  `move_layers`; `ActionSteps.layerOffset` is gone.
- **RAW preset library per entry (12B-2):** `RawPresetLibrary.diagnose` answers one file's fault
  (`unreadable`, `oversize`, `not_json`, `unknown_version`, `invalid_schema`); listing adds `duplicate_name`
  and returns `{entries, broken}`. Library-level faults (folder unreadable, >256 dir entries, >128 `.json`,
  busy `flock`) still throw. `repairable` is a dry run of the same `repairPlan` `repair_raw_preset` uses.
  Repair rewrites canonical schema 2, keeps each `raw` control only if `RawSettingsPatch.decode` accepts it
  alone (dropped keys reported sorted), never rewrites a future schema, refuses a valid entry.
  `delete_raw_preset` takes exactly one of `name` / `file`. v1 → v2 on write is field-for-field identity.
  A NEW-name save onto an occupied canonical digest file is refused (behaviour change from silent overwrite);
  the 128 bound counts `.json` FILES. Library writes stay outside Undo and Actions; applying a preset records
  resolved `apply_raw_settings`. Raw-preset tools are refused at float.
- **Interactive Play prompts:** the Actions window's Play and Play Last pass `ActionFailurePrompt.ask`.
  A failed step whose `on_error` is `stop` asks Stop / Continue only when its command could not run or names an
  object the document does not have; Continue skips the failed step and plays on, Stop keeps the applied steps,
  each its own History state. An error inside a command (a `ToolError` thrown with the typed kind
  `.commandFailed(cause:)`: Move / Transform Selected Pixels on a selection that holds no pixels; the
  transform's pixel-work limit was one until that refusal was removed) puts up an OK-only alert,
  "<step> stopped: <cause>.", and the play stops there. Select Inverse with nothing selected is not a failure:
  `modify_selection` `invert` is a no-op success at every depth; the menu item stays disabled. A `continue`
  step, `stop_on_error`, Batch, `run_action` and ⌃F never ask, and no beep follows an answered prompt
  (`needsAttention`). Messages: "The step “<step>” could not run."; "The step names layer “<name>”, which this
  document does not have." (a `$layers` miss uses the could-not-run sentence); "<step> stopped: the selection
  holds no pixels."; the reason is the secondary text; buttons Stop, Continue left to right, Continue the
  default (Return), the standard Mac arrangement; a command failure offers OK alone. Test record of the
  behaviour: the attended takes `doctored-fixtures/photoshop/action-failure-472/take2-continue` and
  `take3-watched` (identical), with take1's `paste_empty` (receipt
  `doctored-fixtures/photoshop/action-failure-472/take1/job/paste_empty/photoshop.json`): `paste_empty` and
  `select_missing` prompted and Continue played on; `inverse_none` did not fail; `blur_empty` stopped at its
  step; Stop left the earlier step applied (`layers_after` `[AF_before, Background]`) and did not run the later
  one. `action_failure.py` refuses without `--attended`. Not built: filters refusing on an empty selection
  (set aside as post-v1 on 2026-10-10).
- **Per-step history:** `ActionPlayer.run(history:)` takes `ActionRunHistory`. The Actions window's Play
  and Play Last pass `.perStep`: each step runs in `withUndoGroupDiscardingEmpty`, so a step that changed
  something is one History/Undo state under its own tool's undo name and counts its own change; a failed or
  no-op step leaves none; `progress.tick` and `prompt` run outside the groups. `run_action` stays `.oneEntry`
  (`withReplayUndo`) by the MCP rule that each tool call is one undo step (README "Agent edits"); Batch stays
  one entry per file because it saves and closes each document; ⌃F is one entry. `undo`/`redo` stay refused as
  steps. jobs-probe "action failure prompt" (h).

## Rejected alternatives
- Persisting ids in 12B — would bump both format versions for value an in-session id already delivers (done later, additively, in 12E-1).
- Driving the Batch sheet from MCP — a modal session cannot answer tool calls.
- Refusing batch work in `AgentServer.execute` — would refuse the batch's own steps.
- Recording parameters automatically — choosing the knob is a reuse decision, not an observation.
- A UI that turns a literal path into `$asset` in place — packaging is the only route, deliberately.

## Consequences
- New recordable tools with canvas-coordinate arguments must be added to `ActionGeometry.fields` (and
  `canvasAfter` if they change the canvas) or they refuse non-absolute modes; a coordinate-carrying tool
  whose remapping is not qualified goes in `ActionGeometry.unqualified`, so its refusal names that
  rather than claiming it has no coordinates.
- An agent call whose handler resolves its target from a document MODE is pinned in the recorded step
  (`ActionRecorder.pinningModeDefaults`); a new such default goes there.
- New file-naming arguments must be added to the `ActionAssets` table to travel in bundles.
- 12C-1 lifted the per-entry contract to `../app/Sources/PreferenceLibrary.swift` for every preference
  library; repair stays RAW-only.

## Open items (verified 2026-10-07)
- `DONE` — the interactive Play's Stop / Continue
  prompt (Decisions, **Interactive Play prompts**).
- `DONE` — per-step history for an interactive Play
  (Decisions, **Per-step history**).
- `DONE` — which failures prompt (Decisions,
  **Interactive Play prompts**).
- `OPEN` — deferred: geometry modes
  for the retouch recipes (`patch`, `content_aware_fill`) and typed numeric ink, which are not in
  `ActionGeometry.fields` and refuse non-absolute modes; a parameters editor UI beyond the JSON sheet; zip
  `.draction` bundles.
- `DONE` — a recorded pivot for matrix remaps
  (Decisions, **Pivot rule**).
- `OPEN` — a synthetic fixture for the subjects half of
  drive gate B10, which needs `--photos-dir` today.
- `DONE` — the refusal says so honestly: the
  `unqualified` tools and `content_aware_fill`'s `source_mask` answer "supports absolute geometry only", and a
  `content_aware_fill` without one names the selection step.
- `DROPPED` — persisted channel ids
  (`$channel.id` stays session-only): an id would only tell apart same-named channels, and channels resolve by
  name.
- `DONE` — the 12A `color_space` follow-ons ([phase-12a-replay-status.md](phase-12a-replay-status.md)).
- `DONE` — 8D drive re-qualified: its probe checks
  per-entry `broken` rows, `../scripts/verify_phase8d.py` no longer pins the catalog count, and
  `../scripts/raw-probe/native.py` reads the 45/46 identity wrapper.
- `DONE` — persisted layer ids + cross-session `$layer.id` (12E-1/12E-1b, `../core/src/identity_native.rs`, `ActionIdentity` rule 2).
- `DONE` — typed `reorder_layers` and typed locks (12E-1/12E-1b; `DocumentPrecision.swift:113`, `FloatDocument.swift:377`, `../core/src/precision_lock.rs`).
- `DONE` — per-entry diagnosis for non-RAW libraries (12C-1, `PreferenceLibrary.swift`).
- `DONE` — multi-layer transform / clone-heal dabs (12F), Paste `$asset` (12G-2; since 12G Part B a Paste records
  a live step by default, [phase-12g-clipboard-status.md](phase-12g-clipboard-status.md)).
- `DONE` — `add_text`, `place_image` and
  `transform_selection` are in `ActionGeometry.fields`; `quick_mask_stroke` (canvas-size-locked coverage) and a
  coordinate-free `select_subjects` refuse with their own sentences. Drive gate B8.
- `DONE` — mode defaults are pinned in the recorded
  step (Consequences, `pinningModeDefaults`): a no-target `fill_selection` records `target: "quick_mask"` in
  Quick Mask and `"layer"` outside it. `get_warp` and the `assisted_*` tools joined `nonRecordable`. Gate B9.
- `DONE` — a missing `target` is `"layer"`
  at the 16-bit depth gate and at `RetainedImagePolicy.refusal`, as `AgentServer.paintTarget` reads it; a stroke
  aimed at a paintable plane on an adjustment layer records `target: "mask"`; B8 runs every mode on both sizes.
- `DONE` — `select_subjects` `point`/`region`,
  `transform_mask` `matrix` and `transform_layers` `matrix`/`source_rect`/`corners` are in
  `ActionGeometry.fields`. Refused by name: a numeric-string `select_subjects` coordinate at a non-absolute
  replay, and a remap that gives `transform_layers` a non-integer translation in a 32-bit float document. The
  three mask recorders record `{"$layer": "active"}`. Gate B10.
- `DROPPED` — in-place literal-path → `$asset` UI editor (this doc: packaging is the only route, deliberately).
