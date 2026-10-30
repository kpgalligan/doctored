# Phase 12B — Reusable Actions and the RAW preset library

**Status:** BOTH halves shipped 2026-09-18. **12B-1** (items 1–7 below) is a
Rust identity module and its one FFI export, plus Swift; **12B-2** (item 8) is
Swift only. **No `.rz` change and no `RZDC_VERSION` bump at either depth in
either half.**

Plan text this slice implements: *"Package required assets, use stable layer
identities and add explicit absolute/relative/anchor-based geometry and named
parameters. Verify mixed-size batches, partial failures, cancellation and naming
collisions. Add per-entry diagnosis, explicit removal/repair and version
migration for the RAW preset library."*

**12B was two runs.** 12B-1 is items 1–7 and 12B-2 is item 8; each has its own
drive, and both are listed under Verification.

## What shipped

### 1. Stable layer identity, at both depths

`core/src/doc_identity.rs` is the third member of a family this crate already
had twice — `Channel::id` (`doc_channel`) and `Guide::id` (`doc_guide`) — and it
exists for the same reason, only more sharply. A layer's INDEX is renumbered by
every insert, delete, group, ungroup, merge, reorder and duplicate, so an index
recorded at one moment points at a different entry at the next; a NAME is not
unique and a rename breaks it. Phase 12A recorded names alone, so renaming the
layer an action was built around silently aimed every step at whatever else
matched, or at nothing.

- **The model.** `Layer.id: u64` and `PrecisionDocumentLayer.id: u64`, minted
  from ONE `AtomicU64` shared by both depths so an id can never mean two layers
  in one process. Never 0, which is therefore free to mean "no layer".
- **The minting rule, in one line: a NEW picture gets a new id; the SAME
  picture carried through an op keeps its own.** `Layer::new` mints (so every
  insert, Via Copy, Stamp Visible and flatten does); `duplicating_layer` mints
  PER COPIED ENTRY across a duplicated group's whole subtree, and so does the
  typed `duplicate_layer`; a `group_layers` container mints while its members
  keep theirs; a MERGE keeps the LOWEST merged entry's id, because the merged
  entry takes that member's slot, name, locks and link and so IS that layer with
  new pixels. Everything else — a rename, an opacity change, a reorder, a
  geometry op, a render-time materialization, an undo handle — carries the id
  through untouched. `core/tests/layer_identity.rs` asserts each half by name.
- **The surface.** `RzDocument::layer_id` / `layer_index_by_id` and their typed
  twins; `uint64_t rz_doc_layer_id(const RzDocument *, size_t)`; an `"id"` field
  on every `rz_precision_doc_info` layer row; `RasterDocument.LayerInfo.id`,
  `layerRef(_:)`, `channelRef(_:)`, `layerIndex(id:)`; and an `id` on every
  `get_document` layer row.

**Decision — session-scoped, with a NAME fallback, and deliberately NOT
persisted.** This is the guide and channel precedent verbatim (`doc_guide.rs`:
"deliberately NOT persisted"): identity has to hold for as long as a document is
OPEN, so `rz_doc_open` and the PSD reader mint fresh ids, the `.rz` bytes are
unchanged, and no format version moves at either depth. An Action symbol
therefore carries the SESSION it was recorded in
(`ActionIdentity.session`, a UUID per launch, reported by `get_document` as
`identity_session`), and its `id` is honoured only when that token matches —
a stale id is never *tried* rather than being tried and matching a recycled
value. Everywhere else the NAME resolves, exactly as 12A did.

**SUPERSEDED FOR LAYERS by [12E-1](phase-12e-layer-ops-status.md) and
[12E-1b](phase-12e-layer-ops-status.md).** LAYER ids ARE persisted now (the
always-written `LIDS` table), so `$layer.id` resolves across sessions too —
but only where a SECOND fact corroborates it: outside the recording session
the id is honoured only when the record carrying it also carries the recorded
NAME. Ids are unique per DOCUMENT, not per file set, so an uncorroborated id
from another file could aim a step at the wrong picture, and a batch of
DIFFERENT files therefore stays the name path by construction (below). CHANNEL
ids are still session-only, exactly as this paragraph describes, so
`$channel.id` is unchanged.

**A batch is the name path by construction.** Every file a batch opens is a
different document with freshly minted ids, so no stored id can ever match one.
The fallback is not a degraded mode there — it is the only mode, and it is
unchanged behaviour.

**Persisting ids is a later, additive `.rz` wrapper — owner 12E**, with the
stack reorder that needs it. Doing it here would have bumped both format
versions for a feature whose whole value (an action authored minutes ago, run on
the document it was authored against) is satisfied without it.

### 2. Action schema 2

`Action.schemaVersion` is 2 and `readableSchemaVersions` is `{1, 2}`. Schema 1
decodes UNCHANGED: `{"$layer": "Sky"}` is still a name, an absent `geometry` is
still absolute, and a `set_layer_properties` offset step still replays exactly as
it did.

**A file is written at the LOWEST version that can hold it**
(`Action.fileSchemaVersion`): an action with no parameters, no non-absolute
geometry and no schema-2 symbol is still written as `1`. That is what keeps
re-saving an untouched 12A recording — which every Enabled toggle and every
drag-reorder in the Actions window does — producing the same bytes, and keeps it
readable by a build that predates this phase.

Four new symbol forms beside the three schema 1 had:

| symbol | resolves to | fallback |
| --- | --- | --- |
| `{"$layer": {"name", "id", "session"}}` | the layer carrying that id | the TOPMOST layer of that name |
| `{"$channel": {"name", "id", "session"}}` | a channel INDEX | the FIRST channel of that name |
| `{"$param": "<name>"}` | a declared parameter's bound value | — (undeclared is a decode refusal) |
| `{"$asset": "<file>"}` | `assetsDirectory(forActionNamed:)/<file>` | — (missing names the full path) |

### 3. Geometry modes

`ActionStep.geometry: GeometryMode` — `.absolute` (the default, and what every
recording is), `.relative`, or `.anchor(CanvasAnchor)` over `canvas_size`'s own
nine anchor names. Omitted from the file when absolute.

Against a coordinate `v` recorded in a `W × H` space and replayed in `W' × H'`:

- `.absolute` → `v`. 12A's behaviour, unchanged.
- `.relative` → `v · W'/W` (`H` for a y). EVERYTHING scales — positions,
  extents and deltas alike, because "the same fraction of the picture" is what
  proportional means.
- `.anchor(a)` → `f · W' + (v − f · W)`, `f` the anchor's fraction. Distances
  FROM THE ANCHOR are preserved, so sizes and deltas are unchanged and only
  positions move. That is what "keep it 20 px in from the bottom-right" means,
  and it is why the two are separate modes rather than one with a flag.

An integer-recorded value rounds to an Int (a `crop` takes whole pixels);
anything else goes through the four-decimal quantization every recorded
coordinate already uses.

**One measured trap, fixed here and worth knowing about elsewhere.** Swift
bridges an `NSNumber` holding 0 or 1 to `Bool`, so `(0 as NSNumber) is Bool`
is TRUE — and the obvious guard `let n = value as? NSNumber, !(n is Bool)`
therefore silently drops the two commonest integers in the catalog. It made a
`crop` at `x = 0` skip its remap entirely, which looked correct under
`relative` (0 scales to 0) and was wrong under every anchor; the B2 gate's
pixel oracle is what caught it, after the numeric readouts had all passed.
`ActionJSON.isBoolean` / `ActionJSON.number` (`Action.swift`) ask the
CoreFoundation type id instead, and every number guard in this feature goes
through them.

`ActionGeometry.fields` is the explicit per-tool table of which ARGUMENTS are
canvas coordinates, and `unsupported(tool:_:)` refuses a non-absolute mode on
any tool that is not in it — **at decode, by name**. Remapping the wrong
argument is silent (a `tolerance` scaled by 2 is a wrong picture and no error
anywhere), so a setting that would change nothing is a refusal rather than a
no-op. A stroke captured as canonical `native_stroke` dabs is refused too: the
dabs carry their own geometry in a packed blob this layer cannot rewrite, and
remapping only the fallback `points` would replay a stroke that disagreed with
itself.

**Decision — the running recorded space is COMPUTED, not stored.** An action
whose first step is a crop puts every later coordinate in the CROPPED space, so
remapping them against the size the recording started at is wrong by exactly the
crop. `ActionPlayer` therefore carries a running space: it starts at
`recorded_canvas` and advances by `ActionGeometry.canvasAfter` — `crop`,
`image_size` and `canvas_size` name their result, `rotate` swaps the pair for
90/270/−90 and leaves it for 180, everything else leaves it alone — **after a
step SUCCEEDS**; a failed step advances nothing, because the canvas it would
have produced does not exist. `recorded_canvas` keeps its 12A meaning (the first
step's size) and the mismatch warning is unchanged. The B2 drive gate puts a
crop FIRST precisely to measure this: the relative `select_rect` on the
double-size document lands at 2× its recorded values, which is only true if the
space advanced.

A non-absolute step needs `recorded_canvas`; an action carrying one without it
is refused at decode, by step number.

### 4. Named parameters

`ActionParameter { name, type: number|integer|string|boolean, default?,
description? }`, declared on the action, referenced by `{"$param": …}`.
Declaring one is an EDIT, never a recording: the recorder writes what happened
in the values it happened at, and deciding that sigma is the knob and the colour
is not is a statement about reuse.

**`ActionParameters.bind` is the single validator, and it runs before anything
runs.** Every Play path calls it — the Actions window's Play, File ▸ Automate ▸
Play Last, `run_action`, `run_batch` and the Batch window — and
`ActionPlayer.run` calls it again as a defence. An unknown name, a missing
required value and a wrong type each refuse BY NAME with nothing applied; on
success the dictionary holds every declared name with defaults filled in, so no
caller downstream has to ask whether a value is present.

**Decision — a parameter error is an ACTION-level refusal at submission, never a
per-file row.** `run_batch` binds before `BatchRun.start()`, so a bad parameter
is one message before the first file is opened rather than 200 identical rows in
a report nobody would read. The UI asks once, in an
`ActionParametersSheetController` card, for all three Play paths and Batch.

### 5. Asset bundles

`ActionAssets` owns the table of arguments that name a file — `place_image.path`,
`add_live_photo_layer.path`, and `convert_profile.path` / `assign_profile.path`
when `profile == "file"` — and the two directions.

- **`package_action {name, destination, overwrite?}`** writes
  `<slug>.rzaction/` holding `action.json` and `assets/`. Each named file is
  copied and the argument rewritten IN THE BUNDLE to `{"$asset": "<basename>"}`.
  Two DIFFERENT source paths sharing a basename get `logo.png` and `logo-2.png`
  (first free n ≥ 2), each step pointing at its own; the same source path used
  twice is one entry. A missing source file refuses the whole call naming the
  path and leaves NO partial bundle.
- **`import_action {path, name?, overwrite?}`** reads `action.json` (schema 1 or
  2, decode refusals verbatim), saves it through `ActionLibrary.save`, and
  installs `assets/*` into `assetsDirectory(forSlug:)` —
  `<stem>.assets/` beside `<stem>.json`, keyed on the STORED file's stem rather
  than a re-derived slug, because `destination(for:)` takes the next free `-2`
  when two differently-named actions slug the same and two actions must never
  share one assets folder. `rename` and `delete` carry the folder.

**Decision — packaging never modifies the library copy**, so packaging twice
produces the same bundle and a user's own action keeps working after they have
shared it. And **recording stays literal**: a path becomes a `$asset` only
through `package_action` → `import_action`, never behind a user's back.

**`quick_export.destination` is NOT an asset.** It is a per-machine output
FOLDER, not an input the action depends on; copying it into a bundle would be
copying someone's Desktop. The 11A containment rule says the same thing from the
other side — a delivery writes where the person running it chose.

### 6. Headless Batch

`run_batch` / `batch_status` / `cancel_batch` build the same `BatchSettings` the
Setup pane builds and start the same `Timer`-driven `BatchRun`, with no window:
the call returns a `batch_id` immediately, the caller polls, and may cancel at
the next file boundary.

**Decision — the modal is NOT driven.** Driving the sheet would mean running a
modal session from inside a tool call, which cannot answer while it is up.

Every Batch rule is unchanged: the `{name}`/`{n}` template, the containment
test, the claimed-set skip (never an overwrite), the exists-without-overwrite
skip, and the two halves of "stop on error". Preset mode still delivers through
`quick_export`.

**Decision — the exclusion lives in `ActionPlayer.replayRefusal`, not in
`AgentServer.execute`.** A batch's own work reaches `execute` too — every step
of every file's replay, plus the `save_copy` or `quick_export` that delivers it
— so a refusal written at that seam would refuse the batch's own calls and every
file would fail in band. The clause sits behind the existing `isRunningStep`
escape, and `ActionPlayer.dispatchingOwnStep` marks exactly the three calls the
machinery makes on its own behalf (the player's step, `BatchRun.deliver`,
`BatchRun.write`). `AgentServer.swift` gains six `handlers` entries and nothing
else. `BatchRun.allowedDuringBatch` is the small set a caller needs precisely
BECAUSE a batch is running: `batch_status`, `cancel_batch`, `job_status`,
`cancel_job`, `resource_usage`.

### 7. The four layer-set gaps 12A left

| gap | now |
| --- | --- |
| a panel drag that moved a multi-entry selection | **`reorder_layers {layers, at, depth?}`** → `LayerReorder.reorder`, the panel's OWN arithmetic moved out of `LayersPanelViewController+Rows.swift` so the two cannot drift. `at` is the insertion index in the PRE-move numbering — the drop line — because with several subtrees in flight there is no single "already removed" stack to number against. Returns `landed`. |
| a ⇧-click that ADDS to the selection | **`auto_select_layer.mode: "replace" \| "add"`** |
| a lock write over a multi-selection | **`set_layer_lock {layers}`**, one undo step |
| a clipping-mask write over a multi-selection | **`set_layer_clipped {layers}`**, one undo step. **Closed in 12B-2:** the set FOLDS, exactly as the menu twin `applyToSelectedLayers` and the sibling `set_layer_lock` do — an entry the document will not change (at 16 bits, one already in that state) is skipped and listed in `skipped`, and the call is refused, naming the entries, only when NO entry changed. The catalog states the rule. 12B-1 shipped it all-or-nothing, so a recorded mixed-state multi-select clip failed on replay where the gesture succeeded. |

…and the single-layer Move joined the set form on the relative `move_layers`:
it used to record an ABSOLUTE `set_layer_properties` offset, which replayed as a
snap to a fixed canvas position instead of a nudge by the amount dragged.
`ActionSteps.layerOffset` is gone. The 12A `move-record-ui` probe case now
asserts `dx == 40` and that the step names its layer by reference.

**Depth.** `run_batch`, `batch_status`, `cancel_batch`, `package_action` and
`import_action` are in `DocumentPrecision.commonTools` and `FloatDocument.tools`
— none does pixel work of its own, and a batch's steps still meet the depth gate
one at a time. `reorder_layers` was admitted NOWHERE typed and refused by name exactly as
`reorder_layer` was; [12E-1](phase-12e-layer-ops-status.md) shipped the typed
structure ops, so both now answer at 16 bits and at 32-bit float through
`DocumentPrecision.typedTools` and `FloatDocument.tools`.

### 8. The RAW preset library, per entry (12B-2)

One malformed file used to refuse the whole library: `list_raw_presets`,
Develop's preset popup and every save and delete threw "Invalid or unsupported
RAW preset …", and the only repair was finding the file in
`~/Library/Application Support/Rasterize/RAW Presets` and deleting it by hand —
from a folder of SHA-256 file names, where nothing says which preset is which.

- **Per-file diagnosis.** `RawPresetLibrary.diagnose(url)` answers ONE file's
  fault and nothing else, in check order: `unreadable`, `oversize`, `not_json`,
  `unknown_version`, `invalid_schema`. `readListing` runs it over every `.json`
  file and adds the one fault a single file cannot see — `duplicate_name`,
  where the file at the canonical digest name wins and the lexically first wins
  otherwise. A listing is therefore `{entries, broken}`: good presets, and
  `{file, code, reason, repairable}` per bad file. Library-LEVEL faults still
  THROW — an unreadable folder, more than 256 directory entries, more than 128
  `.json` files, a busy `flock` — because none of them names a file a caller
  could act on.
- **`repairable` is a dry run, not a guess.** It is filled in by a post-pass
  once `entries` and the occupied file names are complete, by running the very
  plan `repair_raw_preset` would use (`repairPlan(file:entries:occupied:)`) and
  keeping only whether it threw. The flag and the tool cannot disagree.
- **Repair.** `repair_raw_preset {file}` rewrites one bad file as canonical
  schema 2: the stored name is trimmed and precomposed (that IS the repair for
  the commonest case), unknown top-level keys are dropped, and each `raw`
  control is kept only when `RawSettingsPatch.decode` accepts it ALONE — so an
  unknown key, an out-of-range number, and a flag whose lone value contributes
  no control are each dropped BY NAME into `dropped` (sorted, because
  `RawSettingsPatch.keys` is a `Set`) instead of taking the whole preset with
  them. The file moves to its canonical digest name and the old one is removed.
  An `oversize`, `not_json`, `unknown_version` or `duplicate_name` file is
  refused naming its code; a FUTURE schema version is reported and never
  rewritten, because this build cannot know what it would discard. A file that
  ALREADY lists as a valid entry is refused too ("nothing to repair"): repair is
  not a rename tool for valid presets at non-canonical basenames.
- **Removal by file.** `delete_raw_preset` now takes exactly one of `name` or
  `file` (`required: []` in the catalog, with the rule in the description) —
  a broken entry has no name to ask for. `delete(file:)` validates a bare
  `*.json` basename, re-checks `O_NOFOLLOW` + regular file under the lock, and
  works on any file regardless of validity.
- **v1 → v2 on write.** `RawPreset.schemaVersion = 2` and `RawPreset.json`
  always writes it. The migration is field-for-field identity: the 14 v1 keys
  and the 3 v2 keys keep their key, value and range, an absent field stays
  absent (no default is synthesized and `vignette` is never written neutral),
  and only `schema_version` changes. Reading v1 is unchanged, and
  `Entry.schemaVersion` reports the ON-DISK version, so a listing shows `1`
  until that entry is saved or repaired.
- **No silent overwrite.** A save under a NEW name now refuses when its
  canonical digest file is already held by some other file — a differently
  named valid entry or a broken one — with the same sentence `repairPlan` uses
  ("RAW preset file … is already in use; repair or delete it first").
  **This is a behaviour change:** that case used to overwrite the occupant
  silently. A save that MATCHES an existing name still writes to that entry's
  own file with `replace: true`, touching nothing else. The 128-preset bound on
  a NEW name counts `.json` FILES, not listed entries — the same thing the
  reader bounds — so a folder already at the bound refuses the write instead of
  admitting a 129th file that would then refuse every read.
- **The sheet.** `RawPresetManagerSheet` grows a second popup listing
  `"<file> — <reason>"`, with **Repair** (enabled only for a `repairable` row)
  and **Delete File** beside it; the two MCP tools name it as their UI twin.
  The Develop window's `reloadPresets` is untouched — `entries()` keeps its
  signature and is now `listing().entries.map(\.preset)`.
- **Still preferences.** Every library write stays outside document Undo and
  out of Actions: `repair_raw_preset` is in `nonRecordable` and
  `notAStepTool` beside its siblings, and applying a preset still records the
  resolved `apply_raw_settings`, which is what P5 measures.

**Depth.** `repair_raw_preset` is in `DocumentPrecision.commonTools` beside
`delete_raw_preset` and deliberately NOT in `FloatDocument.tools`: every
raw-preset tool is refused by name at float.

**Scope: RAW only — LIFTED in 12C-1.** `LiveFilterPresetLibrary` and
`ExportPresetLibrary` shared this contract (digest names, advisory `flock`,
atomic publish, bounded reads) but stayed whole-library strict here. **12C-1**
lifted the per-entry diagnosis into `app/Sources/PreferenceLibrary.swift`
([status](phase-12c-workspace-status.md)): the filter, export, metadata-policy
and new document-preset libraries now all report `broken` rows, delete by
FILE, and refuse an occupied canonical slot rather than overwriting it, and
`RawPresetLibrary` keeps its `Fault`/`Broken`/`Entry`/`Listing`/`repair(file:)`
surface as wrappers over the shared types. REPAIR is still RAW-only — only RAW
has a field-by-field recovery to run — so `repairable` is `false` everywhere
else; the shared listing takes the owning library's dry run as that flag.

**A stale probe, recorded not fixed.** `scripts/raw-workflow-probe/main.swift`
(compiled only by `verify_phase8d.py`) asserts the SUPERSEDED whole-library
refusal at its lines 116–121, and `verify_phase8d.py`'s catalog-count assertion
at line 207 was already stale before this run (161 vs 219 tools). Neither is
edited here; **13D** owns re-qualifying that probe. The library API it calls —
`entries()`, `named()`, `save()`, `delete(name:)`, `directory`,
`maximumPresets`, `maximumFileBytes` — keeps every signature.

## Verification

```
cd core && cargo fmt --check
make lint && make test
make typecheck && make app
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase9h_ui.py --require-current \
  --report ~/devel/rasterize-fixtures/phase12-receipts/12b/ui-probe
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase12b_actions.py \
  --app build/Build/Products/Release/Rasterize.app \
  --receipts ~/devel/rasterize-fixtures/phase12-receipts/12b
# 12B-2, and the 12B-1 drive re-run against the same build:
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase12b2_presets.py \
  --app build/Build/Products/Release/Rasterize.app \
  --receipts ~/devel/rasterize-fixtures/phase12-receipts/12b2
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase12b_actions.py \
  --app build/Build/Products/Release/Rasterize.app \
  --receipts ~/devel/rasterize-fixtures/phase12-receipts/12b2/12b-rerun
python3 scripts/generate_mcp_reference.py --url http://127.0.0.1:4917/mcp \
  --output public-docs/src/content/docs/reference/mcp-tools.md --check
```

`scripts/verify_phase12b_actions.py` has SEVEN gates, fixed before the run. Every
claim in it is EXACT — a byte digest, an integer coordinate, a set membership —
so there is no tolerance to widen; the geometry oracle is written out from the
rules in the drive itself rather than read off `ActionGeometry`.

- **B1 identity** — an action whose `layer` is the `{"$layer": {name, id,
  session}}` symbol built from `get_document`'s own ids still aims at the layer
  it was authored against after that layer has been REORDERED, RENAMED, and had
  a same-named sibling added above it; the proof is byte equality with the same
  edit applied by index. The string form and a STALE-session symbol both fall
  back to the topmost layer of that name — a different layer and a different
  digest. A duplicated GROUP hands back ids sharing nothing with the originals.
- **B2 geometry** — one parameterized action (`crop` FIRST, then
  `select_rect` + `fill`, an offset write and a guide) authored absolute,
  relative and `bottom-right`-anchored, run on 96×64, 192×128 and 150×100.
  Every canvas size, selection rect, layer offset and guide position equals what
  the drive computes against the RUNNING space. The relative `select_rect` on
  192×128 lands at 2×, asserted explicitly. And every run is compared PIXEL FOR
  PIXEL against an independent replay of the five predicted calls, because the
  readouts alone cannot see the crop's ORIGIN: an anchored crop takes the
  bottom-right 48×32 of a 150×100 document and an absolute one the top-left,
  and both leave a 48×32 canvas. That is the assertion that caught the
  `is Bool` bridging trap above.
- **B3 parameters** — default used, supplied value used, missing / wrong-type /
  unknown each refused by name with the document's digest unchanged; a
  `run_batch` that will not bind is refused with no `batch_id` and an empty
  output folder.
- **B4 batch** — three sizes plus a garbage `.png` and an `a.png`/`a.jpg`
  collision at `stop_on_error: false`: `wrote` / `failed (could not open)` /
  `skipped (claimed)`, output sizes as B2 predicts, and NO row carrying the
  in-band batch refusal. A preset-mode batch delivers through `quick_export`
  with no refusal. A 12-file run cancelled after `processed ≥ 1`: written equals
  the rows, `not_processed > 0`, and the folder holds no extra files. Mid-run a
  `crop` from the drive is refused in band naming the batch while
  `batch_status` still answers.
- **B5 bundle** — two different-folder `logo.png` sources package to
  `["logo.png", "logo-2.png"]` with two `$asset` symbols and an untouched
  library copy; the bundle is MOVED, imported, both originals DELETED, and the
  run is still pixel-identical. Deleting one installed asset makes the run fail
  naming the full path and `package_action` refuse naming it.
- **B6 old schema** — a schema-1 file the drive writes by hand replays
  byte-identically to its schema-2 absolute twin.
- **B7 gap tools** — `reorder_layers` landed indices, `auto_select_layer
  mode: "add"`, `set_layer_lock` / `set_layer_clipped` with `layers`, and
  `reorder_layers` refused by name at 16 bits.

`scripts/verify_phase12b2_presets.py` has NINE more, on the same terms — every
claim a byte digest, a file name, a code string or an integer index, with the
digest file names computed in the drive from `hashlib` rather than read off the
app. It owns every instance and points all four preference libraries at private
folders.

- **P1 listing** — two good presets (a schema-1 "Sunny" at its canonical digest
  name, a schema-2 "Second" parked at "Collide"'s) plus malformed, 8193-byte
  and `schema_version: 99` files: both presets list with their ON-DISK
  `schema_version` and `file`, the three bad files come back as `broken` rows
  carrying `not_json` / `oversize` / `unknown_version` and `repairable: false`,
  and `apply_raw_preset` of the v1 entry still works.
- **P2 removal and repair** — the advertised `required` lists (`[]` and
  `["file"]`); `delete_raw_preset {file}` clears the oversize file; a refused
  repair names its code and leaves the file's bytes IDENTICAL; a repairable
  file (schema 1 carrying `moire`, an unknown `decoder`, `exposure: 99` and the
  name `" Cloudy "`) lists `invalid_schema, repairable: true` and repairs to
  the canonical digest name as schema 2 with `dropped == ["decoder",
  "exposure"]`; a file named `" Sunny "` lists `repairable: false` and is
  refused naming `duplicate_name`; a `file` with a `/`, `..` or a non-`.json`
  suffix, both spellings together and neither spelling are each refused.
- **P3 migration on save** — replacing the schema-1 entry rewrites ITS OWN file
  with `"schema_version":2` and the identical `raw` object; the listing row
  then reads 2.
- **P4 a bad file blocks nothing** — with the unknown-version file still
  present, `redevelop_raw`, `apply_raw_settings`, `sync_raw_settings` and a new
  `save_raw_preset` all succeed; a save whose name digests to the broken file's
  basename is refused NAMING it; a save whose name digests to "Second"'s file
  is refused and "Second"'s bytes are unchanged; `replace: true` on "Second"
  itself still writes in place.
- **P5 resolved Actions are immune** — a recorded `apply_raw_preset` is ONE
  `apply_raw_settings` step, and a freshly opened copy of the DNG exports the
  same PNG digest after the preset is replaced, after it is renamed, and after
  it is deleted.
- **P6 the clipped fold** — at 16 bits with one entry already clipped,
  `layers: [1, 2]` returns `ok` with `skipped == [2]`, both rows clipped, ONE
  undo restoring; an all-refused set is still refused naming both.
- **P7 the lock no-op echo** — a repeated single-entry `set_layer_lock` answers
  `changed: false` carrying `layer`.
- **P8 the write bound counts files** — a folder of exactly 128 `.json` files,
  one of them bad, refuses a new preset as full and writes nothing while the
  library still LISTS; `delete_raw_preset {file}` on the bad file frees the
  slot and the same save then succeeds.
- **P9 repair only repairs** — `repair_raw_preset` on a VALID entry's file is
  refused and its bytes are unchanged.

### A note on the 9H UI probe

`scripts/verify_phase9h_ui.py --require-current` is re-run here because
`scripts/native-paint-probe/replay.swift` changed (`move-record-ui` now asserts
the relative `move_layers` form for the single-layer drag). **`move-record-ui`
passes in every run.** The six TABLET cases (`pressure-ui`, `airbrush-ui`,
`pixel-airbrush-ui`, `zero-pressure-pixel-ui`, `quick-mask-ui`,
`byte-pressure-ui`) fail with "Tablet conversion changed owned canvas
coordinates" on this machine in its current display state — the probe's own
`NSEvent` → `CGEvent` → `NSEvent` round trip, whose assertion `main.swift`
already documents as "raced rather than wrong". It is NOT this phase's:
stashing every 12B app source and re-running the probe on the pre-12B tree
produces the identical six failures (receipt:
`ui-probe-baseline-pre12b-run.log` beside `ui-probe-run.log`), and one run
earlier in the same session — with the probe edit already in place — passed all
twelve. Every case that synthesizes a tablet event fails and every case that
does not passes, which is the signature of the window-server dependency rather
than of any code here.

## Deliberately NOT in this slice

- **Persisted layer ids in `.rz`** — shipped in
  [12E-1](phase-12e-layer-ops-status.md) as the always-written outermost
  wrapper 45 (byte) / 46 (typed) with its `LIDS` table, and
  [12E-1b](phase-12e-layer-ops-status.md) made Actions honour `$layer.id`
  across sessions when the recorded NAME corroborates it (and migrated comps
  to the same persisted id, `LCMP` schema 2).
- Multi-layer transform and clone/heal dabs — 12F. Paste — 12G. Typed
  `reorder_layers` shipped in 12E-1; typed LOCKS shipped in 12E-1b.
- Geometry modes for the retouch recipes (`patch`, `content_aware_fill`) and
  typed numeric ink; a parameters editor UI beyond the JSON sheet; zip bundles;
  the 12A `color_space` follow-ons; and a UI editor that turns a literal path
  into an `$asset` in place — packaging is the only route, deliberately.
