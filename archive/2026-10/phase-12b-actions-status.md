# Reusable Actions and the RAW preset library (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12B (12B-1 items 1–7, 12B-2 item 8), both shipped 2026-09-18
- **Original:** [archive/phase-12b-actions-status.md](../archive/phase-12b-actions-status.md) — full history, measurements, receipts, drive gates B1–B7 / P1–P9
- **Key commits:** `3e86887` (12B-1), `729524f` (12B-2)

## Context
12A recordings named layers by name and coordinates absolutely, so a rename, reorder or different canvas size
silently re-aimed or broke an Action; assets named by path did not travel; Batch needed a modal window; and a
single malformed RAW preset file refused the whole preset library. Plan text: package assets, stable layer
identities, absolute/relative/anchor geometry, named parameters, batch failure/cancel/collision behaviour, and
per-entry diagnosis/repair/migration for the RAW preset library. No `.rz` change and no `RZDC_VERSION` bump.

## Decisions
- **Stable layer identity** (`../core/src/doc_identity.rs`): `Layer.id` / `PrecisionDocumentLayer.id: u64`
  from ONE process-wide `AtomicU64` for both depths; 0 means "no layer". Rule: *a new picture gets a new id;
  the same picture carried through an op keeps its own* — `Layer::new`, duplicate (per copied entry, whole
  subtree) and a new group container mint; a MERGE keeps the LOWEST merged entry's id; rename, reorder,
  geometry, undo handles carry it. Surface: `rz_doc_layer_id`, `"id"` on typed info rows and on
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
  values round to Int. An affine matrix (`transform_mask`, `transform_layers`) is conjugated by the remap R,
  `M' = R ∘ M ∘ R⁻¹` — relative keeps `a`, `d` and writes `b·sy/sx`, `c·sx/sy`, `tx·sx`, `ty·sy`; anchor keeps
  the linear part and writes `tx + ox − (a·ox + c·oy)`, `ty + oy − (b·ox + d·oy)` with `o = f·(W' − W)` — as
  plain Doubles that are never rounded, so no pivot is needed and the matrix and `source_rect` + `corners`
  forms agree for Double-valued recordings (every UI recording) to the corners' four-decimal quantization
  (≤ 0.00005 px; integer-written corners round half away from zero, up to 0.5 px).
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
- **Asset bundles** (`ActionAssets`): `package_action` writes `<slug>.rzaction/{action.json, assets/}`,
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

## Open items (verified 2026-09-29)
- `OPEN` — geometry modes for the retouch recipes (`patch`, `content_aware_fill`) and typed numeric ink: not in
  `ActionGeometry.fields` (`../app/Sources/ActionGeometry.swift`), so they refuse non-absolute modes. The
  refusal now says so honestly: the `unqualified` tools and `content_aware_fill`'s `source_mask` answer
  "supports absolute geometry only", and a `content_aware_fill` without one names the selection step.
- `OPEN` — a parameters editor UI beyond the JSON sheet, and zip `.rzaction` bundles: not implemented; unowned.
- `DROPPED` — persisted channel ids (`$channel.id` is session-only, `../app/Sources/ActionIdentity.swift`): won't do. An id would only tell apart same-named channels, and channels resolve by name.
- `TRACKED` — the 12A `color_space` follow-ons → carried in [phase-12a-replay-status.md](phase-12a-replay-status.md) open items.
- `DONE` — 8D drive re-qualified: `../scripts/raw-workflow-probe/main.swift` checks per-entry `broken` rows, `../scripts/verify_phase8d.py` no longer pins the catalog count, and `../scripts/raw-probe/native.py` reads the 45/46 identity wrapper (`identity_body`).
- `DONE` — persisted layer ids + cross-session `$layer.id` (12E-1/12E-1b, `../core/src/identity_native.rs`, `ActionIdentity` rule 2).
- `DONE` — typed `reorder_layers` and typed locks (12E-1/12E-1b; `DocumentPrecision.swift:113`, `FloatDocument.swift:377`, `../core/src/precision_lock.rs`).
- `DONE` — per-entry diagnosis for non-RAW libraries (12C-1, `PreferenceLibrary.swift`).
- `DONE` — multi-layer transform / clone-heal dabs (12F), Paste `$asset` (12G-2). A Paste records
  a live step by default (the clipboard at replay; the `$asset` step is MCP `record_payload` / `payload_path`), and
  Copy and Copy Merged are recorded steps
  ([phase-12g-clipboard-status.md](phase-12g-clipboard-status.md)).
- `DONE` — the remaining coordinate tools: `add_text`, `place_image` and `transform_selection` are in `ActionGeometry.fields`; `select_subjects` with a `point`/`region` and `transform_mask` were `unqualified` until the `select_subjects` / `transform_mask` entry below remapped them; `quick_mask_stroke` (canvas-size-locked coverage) and a coordinate-free `select_subjects` refuse with their own sentences. Drive gate B8.
- `DONE` — an agent's no-target `fill_selection` records `target: "quick_mask"` in Quick Mask and `"layer"` outside it, and a stroke forced onto an adjustment layer's mask records `target: "mask"` (`ActionRecorder.pinningModeDefaults`), so each replays onto what it edited or refuses by name; `get_warp` and the `assisted_*` tools joined `nonRecordable`. Drive gate B9.
- `DONE` — a missing `target` is `"layer"` at the 16-bit depth gate (`DocumentPrecision.rasterTools`) and at `RetainedImagePolicy.refusal`, as `AgentServer.paintTarget` reads it (jobs-probe "target default layer"); a stroke aimed at a paintable plane on an adjustment layer records `target: "mask"` (`pinningModeDefaults`, B9); B8 runs every mode on both sizes, so relative geometry is proved at a non-integer 1.5625 scale.
- `DONE` — `select_subjects` `point`/`region` (`.pair`/`.rect`), `transform_mask` `matrix` and `transform_layers` `matrix`/`source_rect`/`corners` (`.affine`/`.rectArray`/`.points`) are in `ActionGeometry.fields`, so the Mask Transform and Free Transform steps get the Geometry row menu. A numeric-string `select_subjects` coordinate is refused by name at a non-absolute replay, and a relative or anchor remap that gives `transform_layers` a non-integer translation in a 32-bit float document is refused by name (`DocumentPrecision`), as intended. The three mask recorders (Transform, Properties, Link with Content) record `{"$layer": "active"}`. Drive gate B10 (subjects half needs `--photos-dir`).
- `DROPPED` — in-place literal-path → `$asset` UI editor (this doc: packaging is the only route, deliberately).
