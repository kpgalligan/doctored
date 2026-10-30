# Phase 12A — Existing replay gaps

**Status:** shipped 2026-09-18. Swift only; no Rust change and no `RZDC_VERSION` bump.

Plan text this slice implements: *"Close remaining multi-layer gesture,
channel/plane and playback-recording gaps. Preserve brush pressure and airbrush
timing or record canonical dab output with equivalent behavior. Add
profile-qualified color arguments for exact wide-gamut sampling/painting."*

Completion-gate clause: *"Replay representative pressure/airbrush/selection/
multi-layer actions on identical inputs with matching output."*

## What shipped

### 1. The multi-layer Move records `move_layers`

A Move drag or nudge over a SET of layers recorded the visible
`__unrecorded("Move Layers")` placeholder, because `set_layer_properties` writes
ONE layer's absolute offset and there was no multi-layer move tool. There is one
now:

- **MCP `move_layers(layers, dx, dy, document_id?)`**
  (`AgentServer+MoveLayers.swift`) — `layerIndices` → `rejectLockedMove` →
  `editStructure { $0.moveLayers(indices, dx:, dy:) }`. One `moveLayers` call, so
  a group's subtree and every linked partner follow and a POSITION lock refuses
  the whole set by name; one undo step. `dx = dy = 0` is reported as an unchanged
  no-op (`structureNoOp`), never an error — a set drag that ended where it began
  still records `move_layers (0, 0)` and must replay cleanly.
- **The step** — `[ActionStep].moveLayersStep(dx:dy:)`, written as
  `{"layers": {"$layers": "selected"}, "dx": …, "dy": …}`, so a replay moves the
  replay document's own selection.
- **The recorders** — `MultiLayerEdit.moveRecord` emits it when the moving set is
  larger than one; `EditorViewController.moveEndRecord(_:delta:)` branches on the
  moving set itself and passes the drag's APPLIED delta (`moveAppliedDelta`, snap
  correction already folded in) to the set branch.

**Decision — the single-layer form is unchanged.** (Superseded by 12B, which
unified the two: both sizes now record the relative `move_layers`, and
`moveEndRecord` passes the drag's delta for both.) One layer still records the
absolute `set_layer_properties` with the note that says so, and
`moveEndRecord`'s one-layer branch is still the literal
`moveRecord(doc, indices, dx: 0, dy: 0)` call it always was. Unifying the
absolute and relative forms belongs to **12B**'s explicit
absolute/relative/anchor geometry, not here. The double-count that a naive
`moveRecord(doc, indices, dx: delta.x, …)` would cause (the drag's ticks have
ALREADY moved the layer, so its offset includes the delta) is why `moveEndRecord`
branches itself rather than passing a delta down: the `move-record-ui` probe case
asserts `offset_x == pre + 40` and that the recorded absolute offset equals the
post-move offset, not twice the drag.

**Depth admission.** `move_layers` is in `DocumentPrecision.commonTools` and in
`FloatDocument.tools`: it is `set_layer_properties`' offset op through
`RasterDocument.moveLayers`, which routes to the typed transform at depth. The
`r5` drive gate MEASURES the answer at 8, 16 and 32-bit float rather than
assuming it, and a refused depth would have to move nothing at all (the gate
asserts that too). **Measured: accepted at all three.** A two-layer float
document moved by `dx: 7, dy: 3` reports both layers at `(7, 3)`, so the tool
stays in `FloatDocument.tools`; nothing was dropped.

### 2. A channel whose name will not read is recorded by INDEX

`channelStep`, `renameChannel`'s panel path and the Channel Options sheet all
recorded `__unrecorded` when `channelInfo(index)?.name` was nil or empty. All
three now fall back to `{"channel": <index>}` through the one helper
`EditorViewController.channelArgument(_:_:)`. `AgentServer.channelArgument`
already accepts both spellings — a real JSON number always means the position —
so the fallback resolves on replay instead of refusing.

Channel DUPLICATE-NAME identity (two channels called "Alpha 1", where the name
form resolves to the first) is **12B**'s: it is the same stable-identity problem
as the layer one, and solving half of it here would have produced a second,
different identity scheme. 12B shipped it as the `{"$channel": {name, id,
session}}` symbol beside the layer one; the index fallback described here is
still what a channel that cannot be read at all records.

### 3. Byte canonical dabs (12F-5 extension)

12A first recorded pressure-size and held-airbrush layer brush/eraser dabs as
RZBR v1. 12F-5 extends the same payload to every byte gesture whose actual
footprint is stamped: clone/heal always, and brush/eraser/dodge/spot healing
when a soft, flowed or non-round tip stamps, including ordinary mouse input.
Pressure-size, pressure-opacity and held airbrush also open byte sessions,
including size-1 pixel and mask/plane/channel/Quick Mask strokes. An unchanged
hard mouse path and historical unversioned tool calls retain their point
footprints.

`ImageCanvasView.finalizeBytePaintStroke` validates the complete session,
serializes canonical dabs and rebuilds the final overlay before publication.
`SoftBrush.rasterizeDabs` handles solid ink, transformed sampled ink and exact
pixel squares; `BytePaintRasterizer` applies the existing selection clip and
one byte coverage-opacity scale. The same rasterizer feeds MCP replay and the
byte Quick Mask RLE adapter. The retouch plate and source matrix, tool options,
target, selection and document-space quantized color are latched at mouse-down;
a mid-stroke options change applies to the next gesture. A 50,001st actual
stamp refuses the whole gesture, rolling back its live preview and leaving no
Undo, dirty state or Action step.

The owned 12F-5 UI source drive passed 50 first-process cases plus shortcut
relaunch, including the nine new named byte cases, exact raw storage and
selection replay, real resource/source/serialization rollback and 14
actual-session cap rollbacks. The built-app drive passed seven generated
canonical tool families, complete native-save comparisons, 33 strict
refusals and comparison with the pinned pre-12F-5 unversioned outputs.
The updated 12A R8 accepts byte mask and pixel canonical Actions while
retaining its float refusal. The current 9H numeric reader and gates passed;
12F-5 is measured qualified, with independent review pending in its handoff.
### 4. Profile-qualified colour

`color_space` is a sibling argument on `brush_stroke`, `fill` and `gradient`:

| value | meaning |
| --- | --- |
| absent / `srgb` | **today's meaning, unchanged**: an authored sRGB hex, converted once into the document's space |
| `document` | the hex IS the document's own bytes — what `sample_color`'s `hex` reports — used verbatim |
| `display_p3` | the hex is a Display P3 colour, converted once into the document's space |

`AgentServer.qualifiedColor(_:_:in:fallback:)`
(`AgentServer+QualifiedColor.swift`) resolves all three and always returns a
document-space colour QUANTIZED to the document's own bytes
(`ColorProfile.quantized`), which is what makes the byte commit and its replay
byte-identical rather than one code apart on a wide-gamut document. An unknown
value refuses by name.

At 16 bits, `precisionInkSpace` answers `.document` for `display_p3` because
`precisionInk(_:document:)` has already converted the ink into the document's
space; `document` there still requires numeric `rgba`, unchanged. The 16-bit
`gradient` parses its own `color_space` (`PrecisionColorTools.swift`) and does
the same thing for its two endpoints, so `display_p3` is accepted at EITHER
depth, exactly as the catalog says. Numeric `rgba`
/ `start_rgba` / `end_rgba` / `fill_mode` stay 16-bit-only and the byte guard
still refuses them.

**Recorder rule.** On a document whose `nsColorSpace != .sRGB`, the brush, fill
and gradient recorders write `color_space: "document"` with the hex built from
`ColorProfile.bytes(color, in: document.nsColorSpace)` — the bytes the tool
itself paints (`ActionArgs.documentHex`, which answers nil on an sRGB document,
so an sRGB recording is byte-for-byte what it always was). Recordings never emit
`display_p3`.

## Inventory: every `.unrecorded` site and its owner

Found by reading every `.unrecorded(` call site in `app/Sources`.

| site | what it is | 12A | owner |
| --- | --- | --- | --- |
| `MultiLayerEdit.moveRecord` (multi-layer Move) | a set drag/nudge | **CLOSED** — `move_layers` | — |
| `EditorViewController+Channels.renameChannel` | a channel whose name will not read | **CLOSED** — index fallback | — |
| `EditorViewController+Channels.channelStep` | the same, for the one-channel commands | **CLOSED** — index fallback | — |
| `ChannelOptionsSheetController` | the same, for Channel Options… | **CLOSED** — index fallback | — |
| `EditorViewController+Actions.transformRecord` (`Transform Layers`) | a multi-layer Free Transform | **closed in 12F-2** | `transform_layers` records exact binary64 matrix/quad, sampler and explicit artwork-rasterization authorization; the owned 12F-2 drive replays the set sample-for-sample with no `__unrecorded` marker and one Undo |
| `strokeRecord` `.clone` / `.heal` with no `strokeCloneSource` | a clone/heal stroke with no latched source | **CLOSED in 12F-5** | source/plate admission cancels before an edit; no `__unrecorded` successful command |
| `strokeRecord` `default:` | a stroke by a tool with no stroke twin | **by design** | unsupported tools never enter the byte stroke commit path; no successful stroke is silently omitted |
| `LayersPanelViewController+Rows` (`Reorder Layers`) | a panel drag that moved a multi-entry selection | left | **CLOSED in 12B** — `reorder_layers` |
| `EditorViewController+Groups` (`Auto-Select (add)`) | a ⇧-click that ADDS to the selection | left | **CLOSED in 12B** — `auto_select_layer mode: "add"` |
| `EditorViewController+Locks` | a lock write over a multi-selection | left | **CLOSED in 12B** — `set_layer_lock {layers}` |
| `EditorViewController+Groups` (clipping mask, multi-selection) | the same shape again | left | **CLOSED in 12B** — `set_layer_clipped {layers}` |
| `ImageDocument.paste` (`Paste`) | the clipboard | left | **12G-2: closed — a `$asset` step.** A recorded Paste writes its landed RZCB payload as a file and records `paste {payload_path}`; `package_action` carries it as `{"$asset"}` and a replay pastes the same samples with no pasteboard ([clipboard status](phase-12g-clipboard-status.md)) |
| `EditorViewController+Actions.geometryRecord` `.resize` | never reached — Image Size records its own two steps | left | by design |
| `ActionPlayer` (`Play "…"`) | a nested replay | left | **by design** — an action may not contain `run_action` |
| `ActionRecorder` (budget exhausted) | the recording hit 1,000 steps / 200,000 points | left | **by design** — the placeholder IS the message |

`ActionCatalogFacts.nonRecordable` (reads, file writers, preset-library writes,
the Actions machinery) and `Action.notAStepTool` are unchanged: every entry there
is a deliberate classification with its reason written beside it, and none of
them is a replay gap.

## Deliberately NOT in this slice

- **Single-layer Move absolute → relative**, channel duplicate-name identity,
  multi-entry Reorder Layers, ⇧-click Auto-Select, multi-selection lock and
  clipping-mask writes — **12B**. All six shipped there
  (`designs/phase-12b-actions-status.md`); the four `.unrecorded` rows above are
  marked CLOSED accordingly, and the `move-record-ui` probe case now asserts the
  relative `move_layers` form for the single-layer drag as well.
- **Multi-layer Free Transform** closed in 12F-2; byte clone/heal/dodge,
  Quick Mask / mask / plane / channel pressure strokes and byte
  pressure-opacity implemented in 12F-5. Its final qualification is tracked
  in the 12F-5 handoff.
- **Paste** — **12G**.
- `color_space` on text, shape, layer-style and `overlay_color` arguments, and
  the 16-bit `document` + hex spelling. A later colour slice.

## Verification

Commands, in the order they were run:

```
cd core && cargo fmt --check
make lint && make test
make typecheck
make app
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase9h_ui.py --require-current \
  --report ~/devel/rasterize-fixtures/phase12-receipts/12a/ui-probe
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase12a_replay.py \
  --app build/Build/Products/Release/Rasterize.app/Contents/MacOS/Rasterize \
  --receipts ~/devel/rasterize-fixtures/phase12-receipts/12a
python3 scripts/generate_mcp_reference.py --url http://127.0.0.1:4917/mcp \
  --output public-docs/src/content/docs/reference/mcp-tools.md --check
```

All eight gates passed (`ALL GATES PASS`), R2 including its `r2b_byte_eraser`
arm.

### Two pre-existing 9H probe faults fixed on the way

Both are in the probe harness, not in product code, and both were failing at
HEAD on this machine before any 12A change (verified by running the probe from a
clean `git worktree` at `a3e4f2c`):

1. `send(_:_:_:pressure:)` asserts that the synthetic
   NSEvent -> CGEvent -> NSEvent round trip preserves window coordinates to
   0.01 px. While the window server is still digesting a just-shown window or a
   just-closed popover it answers about 2.2 px off, so four cases
   (`airbrush-ui`, `pixel-airbrush-ui`, `zero-pressure-pixel-ui`,
   `quick-mask-ui`) failed before any product code ran. `send` now waits,
   bounded at five seconds, for the EXACT answer and then asserts it unchanged
   — the race is removed, the assertion is not weakened.
2. `quickMaskLifecycle`'s cancel timer looked up the progress panel with
   `NSApp.windows.first(where: { $0.title == "Native Brush Coverage" })`. A
   CLOSED progress panel stays in `NSApp.windows`, so once (1) let
   `quick-mask-ui` run its own coverage job, this case latched onto that stale
   panel, read progress 0 for ever and let the job finish uncancelled. The
   lookup now requires `$0.isVisible`.

### UI-source probe (`scripts/native-paint-probe/replay.swift`)

Three cases added to the 9H probe's `cases` list, compiled from the real app
sources and driven with synthetic tablet NSEvents through the real canvas:

- **`byte-pressure-ui`** — an 8-bit document, a pressure-size stroke at
  0.25 / 0.5 / 0.875. Records exactly ONE `brush_stroke` carrying
  `native_stroke` and none of the seven generation fields, with more than three
  distinct resolved diameters. Replayed on an identical FRESH 8-bit document the
  flattened straight-RGBA8 composite is byte-identical; one Undo restores the
  exact pre-stroke owner on the recorded document, and one Undo on the replay
  document restores its own pre-state.
- **`byte-airbrush-ui`** — a held airbrush press, the real 20 Hz Timer. The
  recorded dabs ARE the deposits the timer made, and the same replay equality
  and undo hold.
- **`move-record-ui`** — a one-layer drag of +40 records
  `set_layer_properties` whose `offset_x` equals the post-move `layerInfo`
  offset (pre + 40, NOT doubled); a two-layer drag records one `move_layers`
  step with `dx == 40` and `layers == {"$layers": "selected"}`; replayed on a
  fresh two-layer document the offsets are identical, and one Undo undoes it.

### MCP drive (`scripts/verify_phase12a_replay.py`)

Owned instance on a free port with a dummy key and private Actions, recovery and
preset directories (the `HostApp` pattern). Eight gates, each recording with
`start_recording` / `stop_recording` and replaying with `run_action` on an
identical fresh document, comparing exported PNGs by `hashlib` digest:

R1 byte pressure · R2 byte airbrush (and R2b the same pressure stroke as an
`eraser_stroke`, which takes the erase composite path) · R3 a selection sequence
(`select_rect` → `modify_selection` → `select_ellipse` add → `fill`) ·
R4 a plane stroke and a channel stroke, plus `{"channel": 0}` accepted ·
R5 `move_layers` (replay, `(0, 0)` no-op, locked-layer refusal, admission at 8 /
16 / 32-bit float) · R6 the same pressure stroke on a 16-bit document compared
through a 16-bit PNG export (u16 exact) · R7 profile-qualified colour ·
R8 byte mask/pixel canonical replay and float refusal.

**Tolerances, fixed in the file before anything was measured:**

- R7 document-space round trip: **0 codes**. A document-space hex names the
  document's own bytes; the round trip is exact or it is broken.
- R7 `display_p3` against the lcms (`transicc`) oracle: **max 2 codes, mean
  0.5** — Phase 11C's own matrix-to-matrix tolerance, unchanged. Two cases are
  gated: `display_p3` `#FF0000` on the Display P3 document (the slice's own
  gate) and `display_p3` `#808080` on an sRGB document (a real cross-space
  conversion, in gamut on both sides).

A third measurement — `display_p3` `#FF0000` on an **sRGB** document — is
RECORDED IN THE RECEIPT BUT NOT GATED, and deliberately so. Display P3 red is
outside sRGB: `transicc` reports it unclipped at 110.06 %, while this app clamps
per channel in `ColorProfile.bytes`. Which out-of-gamut clipping policy is right
is an output-colour question (11C's territory), not a replay one; 12A measures it
and writes the number down rather than gating on it or widening a tolerance to
cover it. (It happened to agree exactly — both answer `(255, 0, 0)` — but the
agreement is recorded as an observation, not claimed as a contract.)

**Measured, this run:** every R7 delta is 0 codes. The document-space round trip
reproduced `#3D749E` exactly; `display_p3 #FF0000` on the P3 document and
`display_p3 #808080` on the sRGB document both matched the lcms oracle exactly;
`color_space: "bogus"` refused with "color_space must be srgb, document or
display_p3".

All twelve probe groups passed (`passed: true`, `group_count: 12`), with
`--require-current` clean.

## Receipts

`~/devel/rasterize-fixtures/phase12-receipts/12a/`:

- `ui-probe` — the 9H UI probe report, now carrying the three 12A cases.
- `receipt.json` — the drive's per-gate digests, offsets, refusal texts and the
  R7 measurements against the lcms oracle.
- `work/` — every fixture, export and app log the drive wrote.

## Files

- NEW: `app/Sources/AgentServer+MoveLayers.swift`,
  `app/Sources/AgentServer+QualifiedColor.swift`,
  `app/Sources/AgentCatalog+MoveLayers.swift`,
  `scripts/native-paint-probe/replay.swift`,
  `scripts/verify_phase12a_replay.py`, this document.
- Frozen files, wiring only: `AgentServer.swift` (a `handlers` entry, a 3-line
  `capturedNativeStroke` branch in `paintStroke`, `paintOverlay` internal),
  `ImageCanvasView.swift` (`emitStrokeUpdate` internal, the session line, one
  `commitByteDabs` call), `EditorViewController.swift` (`onMoveEnd` reads the
  applied delta).
- Probe harness, beyond the slice's own cases:
  `scripts/native-paint-probe/main.swift` (the `send` settle above, plus the
  three new `cases` entries) and `scripts/native-paint-probe/quickmask.swift`
  (the visible-panel lookup above).
- `ActionSteps.swift`, `MultiLayerEdit.swift`,
  `EditorViewController+Actions.swift`, `EditorViewController+Channels.swift`,
  `ChannelOptionsSheetController.swift`, `ImageCanvasView+NativeBrush.swift`,
  `AgentServer+NativeBrush.swift`, `AgentServer+ChannelTargets.swift`,
  `AgentServer+PrecisionPaint.swift`,
  `AgentServer+PrecisionScalarEditing.swift`, `SoftBrush.swift`,
  `ColorProfile.swift`, `DocumentPrecision.swift`, `FloatDocument.swift`,
  `AgentCatalog.swift`, `AgentCatalog+NativePaint.swift`,
  `AgentCatalog+Precision.swift`, `scripts/native-paint-probe/main.swift`.
- Docs: `README.md`,
  `designs/professional-photo-editor-implementation-plan.md`,
  `public-docs/src/content/docs/reference/mcp-tools.md` (regenerated).
