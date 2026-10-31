# Replay gaps: multi-layer move, channel fallback, canonical dabs, qualified colour (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 12A, shipped 2026-09-18 (byte canonical dabs later widened by 12F-5, 2026-09-22)
- **Originals:** [archive/phase-12a-replay-status.md](../archive/phase-12a-replay-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-12a-replay-status.md](../archive/2026-10/phase-12a-replay-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `7211b97`

## Context
Recorded Actions still wrote visible `__unrecorded(...)` placeholders for several ordinary gestures (a
multi-layer Move, a channel whose name would not read), pressure/airbrush brush strokes could not replay
identically, and a wide-gamut document could not be sampled and painted back exactly because every hex was
authored sRGB. The plan's gate: replay pressure/airbrush/selection/multi-layer actions on identical inputs with
matching output. Swift only — no Rust change and no `DRDC_VERSION` bump.

## Decisions
- **MCP `move_layers(layers, dx, dy, document_id?)`** (`../app/Sources/AgentServer+MoveLayers.swift`) —
  one `editStructure { moveLayers }` call, so group subtrees and linked partners follow, a POSITION lock
  refuses the whole set by name, and it is one undo step. `dx = dy = 0` is an unchanged no-op, never an error,
  so a drag that ended where it began still replays. The step writes `{"layers": {"$layers": "selected"}}`
  so a replay moves the replay document's own selection. Admitted at 8, 16 and 32-bit float
  (`DocumentPrecision.commonTools`, `FloatDocument.tools`).
- **`moveEndRecord` passes the drag's APPLIED delta** (`moveAppliedDelta`, snap folded in) and never
  re-derives it from offsets — the drag ticks already moved the layer, so a naive delta double-counts.
  (12A kept the single-layer drag on absolute `set_layer_properties`; 12B unified both sizes on relative
  `move_layers`.)
- **A channel whose name will not read records by INDEX** via `EditorViewController.channelArgument(_:_:)`
  (channel steps, rename, Channel Options). `AgentServer.channelArgument` accepts both spellings; a JSON number
  always means the position. Duplicate-name channel identity was left to 12B (`$channel` symbol).
- **Byte canonical dabs (RZBR v1).** Pressure-size / held-airbrush byte strokes record the dabs actually
  stamped (`native_stroke`) instead of generation parameters; `ImageCanvasView.finalizeBytePaintStroke`
  rebuilds the committed overlay from those dabs through the same rasterizer replay uses (`SoftBrush.rasterizeDabs`,
  `BytePaintRasterizer`), so commit and replay cannot diverge. 12F-5 extended this to every stamped byte
  gesture (clone/heal, soft/flowed/non-round tips, mask/plane/channel/Quick Mask); an unchanged hard mouse path
  and historical unversioned calls keep point footprints. Tool state is latched at mouse-down; a 50,001st stamp
  refuses the whole gesture with no Undo, dirty state or step.
- **`color_space` sibling on `brush_stroke`, `fill`, `gradient`**: absent/`srgb` = today's meaning (authored
  sRGB converted once); `document` = the hex IS the document's bytes (what `sample_color` reports), used
  verbatim; `display_p3` = converted once into the document's space. Unknown values refuse by name.
  `AgentServer.qualifiedColor` (`../app/Sources/AgentServer+QualifiedColor.swift`) always returns a colour
  QUANTIZED to the document's own bytes (`ColorProfile.quantized`) — that is what makes commit and replay
  byte-identical on wide-gamut documents.
- **At 16 bits** `display_p3` is converted in `precisionInk` and then treated as `.document`; a `document`
  hex is the document's own bytes there too, each byte v the exact code v·257 (`documentHexInk`, Part A),
  for brush/fill and gradient endpoints alike. Numeric `rgba`/`start_rgba`/`end_rgba` stay 16-bit-only.
- **One gate** (`DocumentPrecision.colorSpaceGate`): `color_space` is admitted on `brush_stroke`, `fill`,
  `gradient`, `fill_selection`, `stroke_selection`, `put_pixels` (added later) and raster `add_text` (8 bits only; the depth tables still
  refuse `add_text` at 16 bits and float first) and refused by name on every other tool at 8 bits, 16 bits and
  float (`refine_selection` keeps its own "unavailable" sentence); one validator (`colorSpaceArgument`) spells
  the unknown-value refusal.
- **Text and shape layer colours take sampled sRGB numbers, not `color_space`** (option 3′; B2 —
  `color_space` on the text/shape tools and `color_runs` — not taken). `sample_color` reports `srgb_rgba`
  (the pixel as continuous extended sRGB, `SRGBReading`, never clipped) and `srgb_in_gamut` at 8 and 16 bits
  (omitted at float); passing `srgb_rgba` as `rgba`, `fill_rgba` or `stroke_rgba` reuses the sampled colour.
  An out-of-sRGB pixel reads outside 0…1, the reply's `note` says so, and the artwork validator refuses it.
- **Layer-style and channel `overlay_color` are won't-do** for `color_space`: style colours are stored by the
  core as 8-bit sRGB, so a document-space colour would need a core and DRDC change; a style colour round-trips
  through the sheet's echo loop as sRGB; and a channel overlay colour is display-only. Both keep refusing
  `color_space` by name and take an authored sRGB hex (`sample_color`'s `paint_hex` at 8 bits).
- **Recorder rule:** on a document whose `nsColorSpace != .sRGB`, brush/fill/gradient recorders write
  `color_space: "document"` with `ActionArgs.documentHex` (nil on sRGB, so sRGB recordings are byte-for-byte
  unchanged). Recordings never emit `display_p3`.
- **Out-of-gamut clipping is not a replay contract.** P3 red on an sRGB document is measured but not gated —
  clipping policy is output-colour (11C) territory.
- **Remaining `.unrecorded` sites are by design:** `strokeRecord default:` (tools with no stroke twin never
  enter the stroke commit path), `geometryRecord .resize` (never reached), nested `run_action` (an action may
  not contain one), and the 1,000-step / 200,000-point recording budget (the placeholder IS the message).
  `ActionCatalogFacts.nonRecordable` / `Action.notAStepTool` entries are classifications, not gaps.

## Consequences
- Any new byte stroke path that stamps real footprints must record canonical dabs and commit through the same
  rasterizer as replay.
- Any new colour-taking recordable tool on a non-sRGB document must record document-space bytes, or it will
  replay one code off.
- A `color_space` on any other tool, silently ignored at 16 bits/float before, now refuses — an Action carrying
  one halts on replay.
- A sampled colour reaches a text or shape layer as `srgb_rgba` numbers; a new artwork colour argument should
  take continuous sRGB numbers too rather than a `color_space` hex.
- Verification drives: the 12A drive `verify_phase12a_replay.py` (R1–R9, R9b, R10; in the private archive
  `kpgalligan/doctored-dev`, not this repository) and the 9H UI-source probe cases
  `byte-pressure-ui`, `byte-airbrush-ui`, `move-record-ui` (`../scripts/native-paint-probe/replay.swift`).

## Open items (verified 2026-10-07)
- `DONE` — the 12A drive's gate
  R9 asserts today's `color_space` tool list, `put_pixels` included (`R9_GATE_WORDS`, `0723cb7`).
- `DONE` — `color_space` beyond brush, fill and gradient:
  the one gate, the 16-bit `document` hex (Part A), `srgb_rgba` sampling for text and shape colours (3′) and the
  two won't-dos are Decisions above. The R9b `convert_precision` replay oracle and R10 are in the 12A drive.
- `DONE` — single-layer Move relative, `$channel` duplicate-name identity, multi-entry Reorder Layers, ⇧-click
  Auto-Select, multi-selection lock/clip writes (12B-1, `3e86887`; see [phase-12b-actions-status.md](phase-12b-actions-status.md)).
- `DONE` — multi-layer Free Transform records `transform_layers` (12F-2); clone/heal/dodge and
  mask/plane/channel byte canonical dabs (12F-5, "Qualified and independently reviewed 2026-09-22",
  [phase-12f-geometry-status.md](phase-12f-geometry-status.md)).
- `DONE` — Paste records a step (12G-2, [phase-12g-clipboard-status.md](phase-12g-clipboard-status.md)): since
  12G Part B a live step by default, which replays the clipboard; the `$asset` step is MCP `record_payload` /
  `payload_path`. Copy and Copy Merged are recorded steps.
