# Phase 12F — high-depth geometry: status

Slice 12F (carryover [P7-28](phase-7-carryovers.md#assignment-register)) is
TEN runs. This doc is written run by run; it is the contract for the ones
that have not shipped as much as the record of the ones that have.

| Run | What it owns | State |
|---|---|---|
| **12F-1** | The two carried PSD predicate fixes; typed CANVAS SIZE at 16 bits and at 32-bit float; the explicit float geometry gate; the Canvas Size retained-source lift | **Shipped 2026-09-20** |
| **12F-1b** | The two carried items — the SUCCESSFUL no-op `canvas_size` at all three depths (byte phantom step included) — and the EXACT canvas ops at 32-bit float: zero-angle crop, the quarter turns, the flips | **Shipped 2026-09-20** |
| **12F-1b2** | Typed STRAIGHTEN (`crop` with a non-zero `angle`) at 16 bits as ONE core op; the Crop tool admitted at depth; an unreadable `angle` refused at every depth | **Shipped 2026-09-20** |
| **12F-1b3** | ARBITRARY canvas rotation at 8 bits end to end (R1–R12, R9's SUPERSEDES note, O1's argument-decidable half); at that run's close, 16 bits and 32-bit float refused a filtered angle BY NAME | **Shipped 2026-09-20** |
| **12F-1b4** | The typed twin at 16 bits (`GeometryEdit::RotateCanvas`), and O1's remainder — the CONTENT-dependent no-ops | **Shipped 2026-09-21** |
| **12F-1c** | Captured native Move drag/nudge, one-raster Free Transform affine/perspective handles, and Crop/straighten handle previews at 16 bits; integer Move at float; typed offset aliases | **Shipped 2026-09-21** |
| **12F-2** | Atomic set/group/adjustment-content geometry; retained original masters under affine/projective and whole-document geometry; replayable multi-layer Free Transform | **Implemented, qualified and independently reviewed 2026-09-21 (`07d9153`)** |
| **12F-3** | The initial persisted warp/deform field | **Qualified and independently reviewed 2026-09-21** (`phase-12f-3/09-review-3.md`) |
| **12F-4** | Source-coordinate geometry (8G `RZLM` remapping, lens distortion, transverse CA) | **Qualified and independently reviewed 2026-09-21** (`phase-12f-4/09-review-3.md`) |
| **12F-5** | Byte paint recordability 12A left open | **Qualified and independently reviewed 2026-09-22** (`phase-12f-5/09-review-3.md`) |

## What already existed at 16 bits before 12F-1

Not re-specified here, and unchanged by this run: numeric whole-document
resize, zero-angle crop, quarter turns and flips with chrome
(`core/src/precision_geometry.rs`, `precision_guide::geometry_chrome`);
numeric per-layer affine/perspective on rasters and masks with linked-mask
carriage and the single retained-image affine rerender; integer
`move_layers`; and the numeric Free Transform sheet
(`PrecisionTransformSheetController`).

What was ABSENT: `canvas_size` at depth (in no tool list;
`RasterDocument.canvasResized` was byte-only), straighten, arbitrary canvas
rotation at EITHER depth, the `.move` / `.crop` tools at depth, and every
canvas-geometry family at float.

---

# 12F-1 — the carried items, typed Canvas Size, the float gate

## Decisions

### C1 — ONE clip-run question on all three sides

> A clip base has a visible clipped member when there is a **VISIBLE SIBLING
> in the contiguous clipped run directly above the base AT THE BASE'S OWN
> LEVEL; a group is a sibling, read through its folder record.**

12E-3d recorded P7-39's import predicate as not being the same condition as
the export's. The finding this run added: the **BYTE EXPORT has the same miss
as the import.** `psd_export::clip_base`'s `above.depth == depth` guard ended
the run at a clipped group's first CHILD, because the byte stack is flat and
children precede their folder record. Both are fixed; `psd_export16::entries`
already walked siblings and is unchanged.

`ExportStyle.clip_base` has exactly TWO readers, both report sentences, and
the fix moves both at 8 bits: P7-39's own sentence
(`psd_export_preflight.rs`) and the BAKED-style clause
`" Layers clipped to it are written above the rasterized effects."`, which an
8-bit baked base under a visible clipped GROUP **newly gains** — the 16-bit
report already said it. **DISCLOSED:** that clause is a report sentence only;
no pixel and no written byte changed, and every committed PSD golden is
byte-identical.

### C2 — "renders" is an ENABLED effect of the four kinds

`style_knockout::has_grouped_interior_effect` is the one predicate both sides
ask: some ENABLED effect whose kind is one of Gradient Overlay, Colour
Overlay, Satin or Inner Glow (`is_grouped_interior`). Neither report renders
a plane, so this is as far as the question can be answered without one.

**Residual, recorded (owner 10C-3):** an enabled effect that contributes
nothing — a colour overlay at opacity 0 — still counts, and still discloses.

### G1 — typed Canvas Size is a generalized Crop

The byte contract `RzDocument::canvas_resize` IS the specification; nothing
about what a padded plane, a moved guide or a moved channel means is written
twice. `(x, y)` is where the OLD canvas's top-left corner lands in the new
one — the byte `origin`.

* every raster and every mask is `WorkKind::Keep` with its offset moved by
  the origin: the SAME allocation, no sample read;
* every saved CHANNEL becomes a new canvas-sized plane holding an exact copy
  at the origin and 0 elsewhere — `doc_channel::padded_channels`' rule,
  expressed through the existing Nearest plane walk with `outside = 0`;
* chrome through `doc_guide::padded_guides` / `padded_origin`;
* lock-EXEMPT, style factor 1, no `.rz` change, no new FFI export (it rides
  `rz_precision_doc_edit`).

TWO deliberate differences from the byte op, both existing typed contracts:
an exact no-op (the same size at origin `(0,0)`) answers `Ok(None)` where
`canvas_resize` answers an identical copy, and an offset leaving `i32`
REFUSES where the byte op saturates. *(12F-1b reduced that to ONE: the byte
op answers `None` for the exact no-op now too — see N1 below.)*

### G2 — float: ONE explicit core gate, and an honest reason

**Finding: nothing in the core refused float canvas geometry on purpose.**
`image_equal`'s `"typed geometry requires RGBA16 storage"` was reached only
when the planned location equalled the layer's own offset, so a float `crop`
with a NON-ZERO origin SUCCEEDED in the core (all `Keep` plus the u16 channel
walk) and only `x == 0 && y == 0` windows erred. That was an accident.

`precision_geometry_canvas::float_gate` is the decision. On a float document
every `GeometryEdit` answers `float_document::refused_sentence(REFUSED_GEOMETRY)`
EXCEPT `CanvasSize`, and an `Affine` whose whole plan is `WorkKind::Keep` —
exactly the integer translation float `move_layers` already sends. It is
asked as the FIRST statement of `geometry_plan` and of
`geometry_with_context` (both ahead of `retained_affine` and the lock gate)
and again right after `plan::build`. `Execution::layers` short-circuits a
`Keep` plane, so `image_equal`'s sentence is an unreachable backstop.

**NO user-visible behaviour changed.** The host tables always refused these
ops (`FloatDocument.tools` has no `crop`, `FLOAT_FAMILIES` no
`geometry_crop`), so only a direct `rz_precision_doc_edit` caller can see the
new refusal. *(12F-1b changed that: `crop`, `rotate` and `flip` are in
`FloatDocument.tools` and `FLOAT_FAMILIES` now — see E1/E4 below.)*

What stays refused splits in TWO, and the `REFUSED` row is worded that way
everywhere: the FILTERED ops (Image Size, straighten, non-integer affine,
perspective) wait on 11F's criterion verbatim, "a resampling oracle on
extended-range input"; zero-angle Crop, the quarter turns and the flips are
EXACT ops that need NO oracle — a Canvas Size shrink is plane-for-plane a
zero-angle crop — and their refusal is a **SURFACE decision**, owner 12F-1b.
*(12F-1b ADMITTED those three, so the row now names the filtered set only —
see E2 below.)*

### G3 — 11E

Canvas Size samples nothing, so it is evaluation-independent and admitted
under `linear_light`. No code was needed.

### G4 — selection

A canvas-size change CLEARS the selection at every depth
(`ImageDocument.applyEdit` → `clearSelectionForGeometry`), the byte
behaviour. The request's "selection carried" is declined for parity.

### G5 — retained sources: Canvas Size is LIFTED at both depths

**Found defect:** `RetainedImagePolicy.validate` named `"resizeCanvas:"`, a
selector that does not exist (the item is `showCanvasSize:`), and
`CanvasSizeSheetController.applyClicked` made no policy check — so at 8 bits
Image ▸ Canvas Size… was ENABLED and COMMITTED on a document holding a
retained source while MCP `canvas_size` refused the same op.

**What that reachable path does to the source, established from the code:
NOT a desync.** The placement is folded lazily —
`Layer::effective_source` / `PrecisionDocumentLayer::effective_source`
compose `layer.offset − rendered_offset` into the source transform on every
read — byte `canvas_resize` clones each layer keeping the `pixels` `Arc`, so
the `Arc::ptr_eq` binding guard still holds, and live-filter masks are
SOURCE-sized and run before the affine, so a canvas translation cannot touch
them. The next rerender lands at the NEW position: the sheet is a WORKING
feature and the MCP refusal was merely conservative.

**Decision (owner-reversed on that evidence): the refusal is LIFTED.**
`canvas_size` left `RetainedImagePolicy.refusal`'s `whole` set and the dead
`"resizeCanvas:"` entry is deleted, so the sheet, the MCP tool and the
recorded Action agree. At 16 bits the LIFT branch was taken: it is one match
arm — `CanvasSize` is exempted from `geometry_plan`'s whole-document
`unsupported` arm — and the retained layer takes the same `Keep` + moved
offset as any raster, so `effective_source`'s pointer guard holds and
`rebuilt` → `build` → `clean_typed_layers` stores the fold. The fallback was
NOT needed: step 2's typed evidence test passed first time.

**DISCLOSED MCP BEHAVIOUR CHANGE:** `canvas_size` on a document holding a
placed image source went from REFUSED to ALLOWED. It is stated in the
`canvas_size` catalog text and in the README. Every OTHER whole-document op —
`image_size`, `crop`, `rotate`, `flip` and their five selectors — keeps
refusing with `rasterizeFirst`, and every other core whole-document refusal
keeps its sentence. Float is n/a: `place_image` refuses there by name.

## What 12F-1 shipped

* **Core.** `core/src/precision_geometry_canvas.rs` (NEW, the `canvas`
  submodule beside `plan`; its module doc IS G1's and G2's contract);
  `GeometryEdit::CanvasSize { width, height, x, y }`; the `Keep`
  short-circuit in `Execution::layers`; G5's `CanvasSize` exemption;
  `geometry_chrome`'s `CanvasSize` arm (the first typed caller of
  `padded_guides` / `padded_origin`); `float_document::REFUSED_GEOMETRY` and
  the reworded `REFUSED` row; `geometry_canvas_size` in `FAMILIES`,
  `FLOAT_FAMILIES` and `float_document::ADMITTED`; `"canvas_size"` in the FFI
  Geometry arm. `psd_group_records::clipped_run_has_visible_sibling`,
  `psd_export::clip_base`'s sibling walk,
  `style_knockout::has_grouped_interior_effect` and
  `ExportStyle.grouped_interior`.
* **Host.** `PrecisionPixelCommand.canvasSize`;
  `RasterDocument.canvasResized`'s typed route with `cropped`'s
  artwork-description capture/restore verbatim; `canvas_size` in
  `DocumentPrecision.typedTools`, in BOTH `families` maps (the DEFAULT
  `["document"]` branch) and in `FloatDocument.tools`; `showCanvasSize:` in
  both selector whitelists; G5's two `RetainedImagePolicy` list edits; three
  accessibility identifiers on `CanvasSizeSheetController`; the `canvas_size`
  catalog sentences.
* **Tests and drives.** `core/tests/precision_canvas_size.rs` (10 tests, the
  byte twin plus a hand-written table, pointer equality, the chrome, the
  refusals, the native round trip, the capability rows, float migration and
  the whole float gate); G5's evidence tests in `core/tests/retained_sources.rs`
  and `core/tests/retained_source_native.rs`; section (h) of
  `core/tests/psd_knockout.rs`; the 9H probe case `canvas-size-sheet-ui`
  (three depths plain, two depths retained); `scripts/verify_phase12f1_geometry.py`
  (gates A1–A8).

# 12F-1b — the successful no-op `canvas_size`, and the exact canvas ops at float

Arbitrary canvas rotation was CUT from this run to a new run, and Block B —
typed straighten — was DROPPED as one unit and re-owned with it. What shipped
is Block E: the two carried items and the exact canvas geometry at 32-bit
float. *(Block B then shipped on its own as **12F-1b2**, below; arbitrary
rotation shipped at 8 BITS as **12F-1b3**, and its typed twin is 12F-1b4.)*

## Decisions

### N1 — a `canvas_size` that would change nothing is a SUCCESSFUL no-op, at ALL THREE depths

**Binding owner decision.** `canvas_size` with the document's own width and
height — whatever the anchor, since the origin is then necessarily `(0, 0)` —
replies `{"ok": true, "changed": false, "note": "Nothing changed: the canvas
is already W × H, so the anchor moves nothing. No undo step was added.",
"document": …}`. No `performGroupedEdit`, no history step, no dirtying. Never
an error.

This is a **behaviour CHANGE at 16 bits and at 32-bit float**, where 12F-1
shipped an ERROR for the same call, and it removes the 8-bit PHANTOM UNDO
STEP 12F-1 recorded as pre-existing. Two halves:

* **CORE.** `RzDocument::canvas_resize` returns `None` for the exact no-op
  (after the size and channel-budget guards), so no identical copy and no
  phantom undo step can be produced by any caller — the purity rule the typed
  twin already followed. `rz_doc_canvas_resize` is NULL for it, and the
  header's NULL list says so.
* **HOST.** `AgentServer.canvasSize` answers the request through the existing
  `structureNoOp` AFTER `width`/`height`/`anchor` are parsed and validated (a
  malformed call still errors) and BEFORE `requireChannelBudget` and
  `performGroupedEdit`. It mirrors the Canvas Size sheet's own early return
  (`Sheets.swift`: `dismiss(self)`, then `guard w != originalWidth || h !=
  originalHeight`), which already made no step and showed no alert at every
  depth and needed NO code change; and it follows the precedents `move_layers`
  (dx = dy = 0), `set_layer_properties`, and the guide, channel, group and
  label tools set.

**The consequence is the reason.** `Action.swift`'s `on_error` defaults to
`.stop` and `AgentServer+Batch.swift`'s `stop_on_error` defaults to true, so
a recorded `canvas_size` step replayed on a document ALREADY the target size
used to halt the whole replay, and a batch normalizing canvas size across
mixed-size inputs halted on the first file that already matched. Both now run
to completion. Stated in the `canvas_size` catalog entry, in the README and
here.

### E1 — the EXACT canvas ops are ADMITTED at 32-bit float

`float_gate`'s admitted list grows by `Crop` and the five quarter
turns/flips. They read no weight:

* a zero-angle **Crop** is every raster and mask `WorkKind::Keep` (the same
  allocation, pointer-equal) plus the u16 channel Nearest walk;
* a **quarter turn or flip** is a Nearest PERMUTATION — `plan::affine`'s
  `exact_form` pins the filter — so an extended-range component, negative or
  above white, is only ever COPIED.

Three small core arms carry it: `precision_sampling::transform_image` gains
an F32 `Nearest` branch (the same `resample_into` walk over `[f32; 4]`,
rebuilt with `from_rgba32_extended`; any other filter on F32 storage refuses
with the float geometry sentence), `precision_geometry::image_equal` gains an
F32 arm comparing `to_bits` (a full-canvas layer flipped lands at its OWN
offset, which is the one way that comparison is reached), and
`precision_geometry_plan::build` charges each RASTER destination TWICE on a
float document, because RGBA32 is 16 B/px where `work` counted the u16 form's
8.

### E2 — the `REFUSED` row shrinks to the FILTERED set

`REFUSED_GEOMETRY` is now `"Image Size, straighten and free transform"`
(12F-1b3 widened that row again by NAME, to `"Image Size, straighten,
arbitrary rotation and free transform"`) and its reason `"a resampling oracle on extended-range input, including negative
components; Canvas Size, zero-angle crop, the quarter turns and the flips are
exact and admitted"`. `geometry_crop`, `geometry_rotate` and `geometry_flip`
join `FLOAT_FAMILIES` (target `["document"]`) and `float_document::ADMITTED`.

### E3 — STRAIGHTEN is the one half of `crop` a float document refuses

The family table cannot tell a straighten from a crop — both are
`geometry_crop`, and only the ANGLE says whether a sample is resampled — so
`DocumentPrecision.refusal`'s float branch asks
`DocumentPrecision.cropAngle(arguments) != 0` before the family table and
answers `FloatDocument.straightenRefusal`. `cropAngle` is 0 when the caller
named no angle and `.nan` when a value is present but unreadable, so an
unparseable angle is never read as "no straighten"; the 16-bit gate reads the
same helper and its sentence and position are unchanged.

### E4 — the host surface

`crop`, `rotate` and `flip` join `FloatDocument.tools`; `rotateCW:`,
`rotateCCW:`, `rotate180:`, `flipH:`, `flipV:` and `cropToSelection:` join
the float selector whitelist in `EditorViewController+Precision.swift`.
`resizeImage:` and `freeTransform:` stay out — they filter.
`RetainedImagePolicy.refusal`'s `whole` set is UNTOUCHED: `crop`, `rotate`
and `flip` still refuse with `rasterizeFirst` on a document holding a
retained source (owner 12F-2), and a float document holds none.

No Swift ROUTING changed: `RasterDocument.cropped`, `rotated90/180/270` and
`flippedH/V` already branch on `isHighDepth`, which a float document is.

## What 12F-1b shipped

* **Core.** `RzDocument::canvas_resize`'s exact-no-op `None` and the
  `rz_doc_canvas_resize` contract sentence; `float_gate`'s three new admitted
  arms and the module-doc G2 rewrite; the F32 arms in `transform_image` and
  `image_equal`; the float raster charge in `plan::build`;
  `REFUSED_GEOMETRY` and its reason; `geometry_crop` / `geometry_rotate` /
  `geometry_flip` in `FLOAT_FAMILIES` and `ADMITTED`; the header's
  `rz_doc_canvas_resize` NULL list and CANVAS SIZE paragraph (TWO deliberate
  differences → ONE, and the new float sentences).
* **Host.** `AgentServer.canvasSize`'s `structureNoOp` pre-check (six lines
  and a comment in the frozen file); `DocumentPrecision.cropAngle`, the float
  straighten refusal and the three new `floatCoreRefusal` families;
  `FloatDocument.straightenRefusal`, the three tools and the reworded
  `limitation`; the six float selectors; the `canvas_size`, `crop`, `rotate`
  and `flip` catalog sentences.
* **Tests and drives.** `core/tests/float_exact_geometry.rs` (NEW, 6 tests:
  pointer-equal crop, the five hand-written permutations over rasters, masks
  and channels with the PPI transposition and the chrome, the self-equal flip
  answering `None`, the v40 round trip, the capability rows and what still
  refuses); `core/tests/precision_canvas_size.rs`'s byte-twin no-op assertion
  and its narrowed float-gate list; `scripts/verify_phase12f1b_geometry.py`
  (NEW, gates E1–E7); `verify_phase12f1_geometry.py`'s rewritten A4; the 9H
  probe case `float-exact-geometry-menu-ui` and the unchanged-size Apply
  sub-check inside `canvas-size-sheet-ui`.

# 12F-1b2 — typed straighten (`crop` with `angle`) at 16 bits

Block B, shipped ALONE. Arbitrary canvas rotation was NOT started and moved
on to 12F-1b3 with R1–R12, R9's SUPERSEDES note and observation O1 verbatim;
12F-1b3 then shipped the whole 8-bit op and left the typed twin to 12F-1b4.

`crop` with a non-zero `angle` on a 16-bit document is ONE typed core op
equal to the byte composition `doc_align::straighten_layers(m)` +
`doc_channel::transform_channels(m)` + `doc::crop(x, y, w, h)`, reachable
from MCP, from a recorded Action and from the Crop tool, in one undo step. A
32-bit float document keeps refusing by `FloatDocument.straightenRefusal`.

## Decisions

- **G6 and G7 stand verbatim** (the Block-B contract below) EXCEPT G6's last
  sentence, *"No UI: the Crop tool stays disabled at depth (12F-1c)"*, which
  S2 supersedes.
- **S1 — ONE plan.** Every `Content::Raster` and EVERY mask (linked,
  unlinked, group, adjustment) is `plan::affine(source, T(−x, −y) ∘ m,
  filter, rgba)`, where `T(−x, −y) ∘ m` is `m` with `tx −= x, ty −= y`. That
  integer shift COMMUTES with `bounds_of_corners`' snap/floor/ceil, so each
  destination extent equals the byte "transform, then crop" answer exactly,
  and nothing is trimmed (byte `crop` retains off-canvas storage too).
  `exact_form` still gives `Keep` / Nearest for identity, integer translation
  and quarter-turn matrices. Channels resample STRAIGHT from the old canvas
  into `(0, 0, width, height)` with the same inverse — which IS
  `transform_channels` followed by `cropped_channels`, the window being
  inside the canvas — with the filter pinned to Nearest when `exact_form(m)`
  is `Some`, and `Keep` only for the identity at the full window. The Scale
  Effects factor is `sqrt(|det m|)` on RASTER records only (the byte twin is
  `transform_layer_unlocked`). Chrome is `cropped_*` only. The PPI is never
  transposed.
- **S2 — the Crop tool is ADMITTED at 16 bits.** `DocumentPrecision.allows`
  answers `.crop` true when `isHighDepth && !isFloat`; a float document is
  unchanged. The tool is DISPLAY-ONLY until commit (the canvas rotates the
  display image behind a `CGRect` box), and the commit is the numeric typed
  op. 12F-1c's bullet is amended accordingly: it no longer owns `.crop`
  admission or the Crop commit, and KEEPS the REAL-handle drag probe,
  preview-vs-commit ≤ 1 8-bit code, the moved-under-it refusal and all of
  Move / Free Transform.
- **S3 — ONE host composition, NON-ZERO ANGLE ONLY.**
  `RasterDocument.straightenedCrop` (`CropTool.swift`) is the single
  straighten-then-crop both sites call; it routes typed on its FIRST line and
  otherwise holds the byte chain verbatim. The two sites are NOT shaped alike
  and BOTH keep their own angle-0 route: the agent handler keeps its early
  `guard angle != 0` → `cropped`, and `commitCropSession` keeps its one
  closure with the call inside `if angle != 0` and the fall-through
  `cropped`. **Angle 0 NEVER enters `straightenedCrop` at any depth** — at 8
  bits `doc_align::straighten_layers` answers `None` "when nothing would
  change at all", so an ordinary Crop-tool commit would beep; at 16 bits the
  typed op bypasses `RasterDocument.cropped`'s artwork-description
  capture/restore, so a text layer would silently lose its description.
- **S4 — "lock" means the lock EXEMPTION.** A Lock-All layer straightens, as
  the byte twin does: whole-canvas ops fall to `geometry_with_context`'s
  ungated `_ => None` arm. Retained sources refuse through `geometry_plan`'s
  existing `unsupported` `_` arm and float through `float_gate`'s existing
  `_ => refused()` arm. No new code in any of the three.
- **S5 — both tables.** `geometry_straighten` joins core
  `precision_capability::FAMILIES` and the host `coreRefusal` mapping (the
  DEFAULT `["document"]` branch). It is deliberately NOT in `FLOAT_FAMILIES`
  or `float_document::ADMITTED`: `capability("geometry_straighten",
  "document", 32)` is unsupported.
- **S6 — 11E.** A destructive storage edit, admitted under `linear_light`
  like byte straighten and typed `transform_layer`. No code, one assertion.
- **S7 — an unreadable `angle` refuses at EVERY depth.**
  `AgentServer+Retouch.crop` reads `DocumentPrecision.cropAngle(a)` (0 when
  absent, `.nan` when present-but-unreadable — the same NSNumber/String parse
  `doubleArg` performs) instead of `doubleArg(a, "angle") ?? 0`, so the
  EXISTING `guard angle.isFinite, abs(angle) <= 45` answers *"angle must be
  between -45 and 45 degrees"* at 8 AND 16 bits with 0 history steps, and
  float keeps refusing earlier with the straighten sentence. Without this,
  deleting the 16-bit gate would have turned today's refusal of
  `crop {angle: "abc"}` into a silent zero-angle crop WITH a history step.
  `cropAngle` stays the ONE angle parser: both depth gates and the handler
  read it.

  **Disclosed 8-bit behaviour change.** A malformed `angle` used to crop
  silently at 8 bits and now REFUSES by name. **Recorded Actions are
  unaffected:** `ActionSteps.crop` writes `angle` as `ActionArgs.number`
  (a `Double`) only when it is non-zero and omits the key otherwise, so a
  saved step can never carry an unreadable angle — asserted by the drive's
  b3 (`list_actions` shows a JSON number) and by the probe.

## What shipped

- **Core.** `GeometryEdit::Straighten { matrix, filter, x, y, width, height }`
  (serde tag `"straighten"`, all six keys REQUIRED, unknown keys refused);
  `straighten_window` / `straighten_plane` / `straighten_channel` and the
  "# G6 — Straighten" section in `core/src/precision_geometry_canvas.rs`;
  four dispatch arms in `precision_geometry_plan.rs` (595 lines after `cargo
  fmt`, under the 600 cap — all the logic is in `canvas`);
  `geometry_chrome`'s Straighten arm (`cropped_*`); `FAMILIES` +=
  `("geometry_straighten", &["document"])`; `"straighten"` in
  `ffi_precision_edit`'s Geometry arm; the header's "Native STRAIGHTEN"
  paragraph. Carried item 2 is done with it: `image_equal`'s F32 arm compares
  `to_bits` iterators instead of allocating two `Vec<u32>` per chunk.
- **New sentence.** `"typed straighten window must lie inside the current
  canvas"`. NOT a parity gap: the byte chain refuses the same window, because
  the straighten leaves the canvas the size it was and `doc::crop` answers
  `None` when `x + w > width` or `y + h > height` — the typed op merely names
  it, and `core/tests/precision_straighten.rs` asserts the byte twin's
  `None` beside it. Everything else reuses an existing sentence.
- **Host.** `PrecisionPixelCommand.straighten`,
  `RasterDocument.precisionStraightened`, the shared
  `RasterDocument.straightenedCrop`, the `crop` handler's typed route and its
  `cropAngle` read, the DELETED 16-bit gate, the `coreRefusal` angle-picks-
  the-family mapping, `allows(.crop)` and the catalog sentences.
  `AgentServer.swift`, `EditorViewController.swift`, `ImageCanvasView.swift`,
  `Sheets.swift`, `ActionSteps.crop`, `ActionGeometry`, `RetainedImagePolicy`
  and `FloatDocument` were NOT touched.
- **Tests, probe and drive.** `core/tests/precision_straighten.rs` (NEW, 12
  tests with an f64 inverse-mapping resampler written in the file);
  `precision_canvas_size.rs`'s float refused list and
  `float_exact_geometry.rs`'s (carried item 3, a 90° per-layer `affine`) each
  gain one entry; the 9H probe case `straighten-crop-tool-ui`;
  `scripts/verify_phase12f1_geometry.py` gates b1–b5, with
  `--skip-straighten`, `BLOCK_B_CRITERION` and the NOT RUN loop DELETED
  (carried item 1).

**Test (c)'s exact 90° is CORE-ONLY.** It is unreachable from the UI (the
Straighten field clamps to −45…45, `EditorViewController+Crop.swift`) and
from MCP (`abs(angle) <= 45`), so it has no drive or probe twin and is
asserted in the core test alone.

## Numbers

| Measurement | Tolerance, fixed before measuring | Measured |
|---|---|---|
| b1 16-bit straighten vs the 8-bit call, saved pictures (bilinear) | ≤ 1.0 8-bit-equivalent code | 0.498 alpha, 0.498 RGB |
| b1 same, bicubic | ≤ 1.0 | 0.498 alpha, 0.498 RGB |
| b1 non-vacuity control vs the angle-0 crop | ≥ 32 codes | 243.7 (bilinear), 252.9 (bicubic) |
| b2 saved channel, `coverage16 ÷ 257` vs `coverage`, 936 window pixels | ≤ 1.0 | 0.486 |
| b2 channel control vs the angle-0 crop | ≥ 32 codes | 255.0 |
| b3 `.rz` save → reopen → resave | EXACT | SHA-256 identical |
| b3 recorded `angle` | a JSON NUMBER equal to 7.5 | 7.5, number |
| b3 `run_action` replay | sample-for-sample equal | equal |
| core (d) filtered rasters vs the file's f64 reference | alpha ≤ 1/65535 everywhere, RGB ≤ 1 where the reference alpha ≥ 1 | within |
| core (e)/(f) masks and channels vs the scalar reference | ≤ 1/65535 | within |

`render` answers a 16-bit document with its 8-bit DISPLAY projection, so b1
compares the SAVED pictures (16-bit against 8-bit) for the cross-depth rule
and records both depths in its receipt; the display projections are compared
too and agree exactly.

## Receipts

`~/devel/rasterize-fixtures/phase12-receipts/12f-1b2/` —
`12f1_geometry/receipt.json` (A1–A8 and b1–b5), `12f1b_geometry/` (E1–E7),
`9h.json` (31 passed groups + pass 2: 1, `straighten-crop-tool-ui` in
`required_cases`), `8a.json`, and the ten 12D/12E drive folders.

# 12F-1b3 — ARBITRARY canvas rotation at 8 bits

Image ▸ Rotate Arbitrary…, MCP `rotate` with any finite `degrees` plus a
`sampler`, and a recorded `rotate` step, all backed by ONE pure core op with
an EXPANDING canvas (`core/src/doc_rotate.rs`). Exact quarter turns are
delegated bit for bit, an identity is a successful no-op at every depth, and
a 16-bit or 32-bit float document refuses a FILTERED angle by its own
sentence. The typed twin is **12F-1b4** (criterion verbatim under "Open, with
owners"); THE PERMITTED CUT WAS TAKEN and nothing else was dropped.

## Decisions (R1–R12 as 12F-1b left them, re-verified against the tree)

- **R1 stands.** `[cos, sin, −sin, cos]` is `CGAffineTransform(rotationAngle:)`,
  is `rotate90`'s `[0, 1, −1, 0]` and is `ViewRotation`'s measured `R(θ)`.
  Positive is CLOCKWISE. `ViewRotation.normalize`'s formula is written ONCE
  more in the core (`doc_rotate::normalize`) and handed back to the host
  through the plan export, so no host writes it again.
- **R2 stands, with its consequence stated.** On each axis `W′ ≡ W (mod 2)` —
  the box is symmetric about the canvas centre — so the rotation centre keeps
  its pixel phase and the origin is exactly `o = ((W′−W)/2, (H′−H)/2)`, an
  integer. `W′` may exceed `ceil(W|cos| + H|sin|)` by 1, and `W′ < W` is
  ORDINARY: 100×10 at 80° is 28×102 with `left = +36`.
- **R3 stands and is WHY delegation is mandatory, not an optimisation.** R2
  applied to an exact quarter turn of a `W+H`-odd canvas gives the wrong
  size — 63×48 at 90° would be 49×64 rather than 48×63, a half-pixel resample
  of a lossless permutation. "Exact" is `doc_transform::exact_form` on the
  LINEAR part built from the NORMALIZED angle (`EXACT_EPSILON` 1e-9, so
  `|Δθ| ≲ 5.7e-8°`). Delegation happens TWICE: in the core
  (`rotate_canvas` answers `rotate90/180/270()` verbatim — pixels, masks,
  channels, offsets, canvas, `geometry_guides`/`geometry_origin`, PPI
  transposed) and in the HOST (a quarter-classified angle runs today's
  `applyingDocumentGeometry(op)` path, so text, shape and Live Photo
  descriptions COMPOSE and stay editable and the recorded step keeps today's
  INTEGER form). **Disclosed discontinuity:** 90° permutes the guides and
  transposes the PPI; 89.999° shifts and drops guides (R6) and leaves the PPI
  alone. They are two different operations and they look it.
- **R4, R5, R6, R7, R8, R9 stand.** Uncovered corners are transparent, masks
  take their own outside coverage and channels 0; there is no Background KIND
  (P7-38); guides ride `padded_guides` / `padded_origin` by `o`; the
  selection is cleared; one filter serves every plane and the default is
  `bicubic`; each channel is resampled STRAIGHT from the old canvas into
  `(0, 0, W′, H′)` — R9's SUPERSEDES note, which is why no coverage inside
  the new canvas is lost when an axis shrinks.
- **R10 stands, and the byte CORE refuses too.** `straighten_layers` would
  RERENDER a retained source through `SourceEdit::Transform` under a canvas
  that is itself changing size — untested, owner 12F-2 — so `rotate_canvas`
  answers the typed straighten's sentence verbatim, *"rasterize embedded
  sources before this document or perspective transform"*, on the Filtered
  route. The host refuses earlier: `"rotateArbitrary:"` joins
  `RetainedImagePolicy.validate`'s `whole` set, and the MCP `whole` set
  already held `rotate`.
- **R11 lands at the HOST** (the byte core never interprets `meta`):
  `RasterDocument.rotatingCanvas` drops each `described` layer's meta inside
  the same edit, as `straightenedCrop` does. MCP reports `rasterized_layers`
  + `note`; the sheet asks once, with a modal alert. Byte adjustment layers
  are rasters whose `meta` is an OP, so they are kept.
- **R12 lands NOW.** `REFUSED_GEOMETRY` is
  `"Image Size, straighten, arbitrary rotation and free transform"`. 12F-1b
  named "straighten" before a core op existed; this is the same precedent.
  The float refusal a CALLER meets in this run is the host's
  `FloatDocument.rotateCanvasRefusal`, because there is no typed op to reach
  the core gate with yet.
- **N2 — what rides unchanged.** Layer ids (`doc_identity`), locks and links
  (the op is lock-EXEMPT — `straighten_layers`' reason; a Lock-All layer
  rotates), labels, blend modes, opacity, clip flags, layer comps (cloned,
  positions NOT remapped — exactly what `geometry` / `crop` /
  `canvas_resize` do), styles including knockout (the factor is
  `sqrt|det| = 1`, so `scaled_style` returns the SAME `Arc`) and
  `linear_light` (a storage edit, admitted like byte straighten). No `.rz`
  change, no `GeometryEdit` variant, no typed code path, no RGBA8 transform
  plate.
- **N3 — ONE parser.** `DocumentPrecision.rotateDegrees`: 90 when `degrees`
  is absent (today's default), `.nan` when present but unreadable. Both depth
  gates and the handler read it, and the GATES act only on a Filtered
  classification, so an unreadable angle reaches the handler and refuses with
  ONE sentence at all three depths and 0 steps.
- **O1's argument-decidable half is DECIDED and SHIPPED.** A `rotate` by any
  angle classified Identity (0, ±360, 720) answers `structureNoOp` —
  `changed: false`, a note, no step, no dirtying — at 8, 16 AND float, and so
  does a zero-angle `crop` to the FULL canvas (which mirrors
  `commitCropSession`'s own silent return). O1's remainder — the
  CONTENT-dependent no-ops — is re-owned to 12F-1b4 with a criterion.

## Disclosed MCP behaviour changes

Three, all consequences of N3 and of `degrees` becoming a `number`:

| Call | Before this run | Now |
|---|---|---|
| `rotate {degrees: "abc"}` | read as 90 and rotated SILENTLY (`intArg … ?? 90`) | refuses: *"degrees must be a finite number of degrees; positive is clockwise"* |
| `rotate {degrees: 90.7}` | truncated to 90 by `intArg` | rotates by 90.7 and expands the canvas |
| `rotate {degrees: 0 / 45 / 360 / −270 / 450}` | error, *"degrees must be 90, 180, 270, or -90"* | 0 and 360 are successful no-ops; 45 rotates; −270 and 450 are quarter turns |

Recorded Actions are unaffected by the first two: `ActionSteps.rotate` writes
an `Int` and `ActionSteps.rotateCanvas` an `ActionArgs.number`, so a saved
step can never carry an unreadable angle.

**One UNDISCLOSED limit, disclosed here.** After a recorded ARBITRARY
rotation, later geometry-bearing Action steps in `relative` or `anchor` mode
are remapped per axis against the expanded `W′ × H′` canvas — the same
meaning they have after any canvas-changing step — but the recorded space is
no longer an axis-aligned copy of the original picture, so on a
DIFFERENT-SIZE target those rectangles do not follow the rotated content.
There is no rectangle that could; the limit is stated rather than worked
around.

## What shipped

- **Core.** `core/src/doc_rotate.rs` (NEW, 354 lines, cap 600): `RotationKind`,
  `RotatedCanvas`, `rotation_kind`, `rotated_canvas` and
  `RzDocument::rotate_canvas`. It performs NO pixel arithmetic of its own —
  layers through `doc_align::straighten_layers`, channels through
  `doc_transform::resample_mask_with_context`, chrome through
  `doc_guide::padded_*`, the budget through `doc_channel::channels_fit_at`
  and the structure gate through `doc_group::validated`, every one of them an
  unchanged byte contract.
- **FFI.** `core/src/ffi_rotate.rs` (NEW): `rz_canvas_rotation_plan`, which
  takes NO handle — the sheet, the MCP handler and
  `ActionGeometry.canvasAfter` each need the rule with no document in hand —
  and `rz_doc_rotate_canvas`. Header: the six `RZ_ROTATION_*` codes and a new
  block after `rz_doc_canvas_resize`, plus sentences in the channel, guide,
  PPI and float-FILTERED-set passages.
- **New sentences.** `"rotation degrees must be finite"`, `"the rotated
  canvas would exceed the supported canvas size"`, `"the rotated canvas would
  exceed the saved-channel pixel budget"`, `"a layer's rotated extent is
  empty, outside the i32 offset range or past the pixel limit"`,
  `"unsupported rotation filter"`. The retained-source refusal REUSES the
  typed straighten's sentence.
- **Host.** `CanvasRotation.swift` (the plan value type, the one unreadable
  sentence, `isFiltered`, `rasterizedNote` and
  `RasterDocument.rotatingCanvas`), `RotateCanvasSheetController.swift`,
  `EditorViewController+RotateCanvas.swift`,
  `AgentServer+RotateCanvas.swift` (the whole `rotate` handler, which LEFT
  the frozen `AgentServer.swift`), `RasterCore`'s two wrappers,
  `DocumentPrecision.rotateDegrees` / `rotateCanvasRefusal` and the two
  depth gates, `FloatDocument.rotateCanvasRefusal`,
  `RetainedImagePolicy`'s `"rotateArbitrary:"`, `ActionSteps.rotateCanvas`,
  `ActionGeometry.canvasAfter`'s core-driven `rotate` arm, the `crop`
  full-canvas no-op and one `AppDelegate` menu item.
  `EditorViewController.swift`, `ImageCanvasView.swift` and `Sheets.swift`
  were NOT touched.
- **The sheet has NO live preview, and that is a decision.** An expanding
  canvas cannot be shown through `canvas.image` without a transform plate
  this run deliberately does not build; what the sheet shows instead is the
  exact answer a preview would have given — the new canvas size, read from
  the core's plan through the `rotate-canvas-size` label whenever the angle
  field or the direction control reports a change (the angle field is not
  continuous, so it reports on Return or when editing ends; the sampler does
  not enter the plan).
- **Carried items, all four.** The f64 inverse-mapping reference moved out of
  `precision_straighten.rs` into `core/tests/common/inverse_mapping.rs`,
  generic over the maximum code, with u16 wrappers left behind so none of
  that file's twelve test bodies changed (1337 lines, cap 1500);
  `straighten_channel`'s dead `is_finite` check is gone;
  `EditorViewController+Crop.swift` lost its unreachable `guard !fullCanvas`
  and its `try?`, which is now a captured `failure` presented through
  `document.presentError`.
- **Tests, drive and probe.** `core/tests/canvas_rotation.rs` (NEW, 12 tests
  measured against the SHARED reference at 255 and against this file's own
  restatement of R2's box); the null sweep in `doc_tests.rs`;
  `scripts/verify_phase12f1b3_rotation.py` (r1–r8, its own Python f64
  reference); the 9H probe case `rotate-canvas-sheet-ui`.

### Test hardening (owner decision, recorded in this run's fix pass)

Nothing this run wrote is reachable from the three cancellation tests below,
but the once-seen `phase8e_coverage` flake was diagnosed here and the fix was
made an owner decision, so it is recorded with this slice. All three share
ONE shape: `live` is sampled (`progress()` above a threshold AND the worker
not finished) and only THEN is the job cancelled, so a worker that finishes
in that two-statement gap succeeds on a window that looked live and the live
arm's "must have been cancelled" assertion loses the race. The fix is the
same in each: judge the OUTCOME rather than the sample, with the check TOTAL
for both outcomes — success ⇒ every sample written / nothing reported,
refusal ⇒ the message contains "cancel" and nothing was published — and count
the attempt as live only when the outcome was the refusal (`observed_live =
live && result.is_err()`), so a worker-won attempt RETRIES on the next, bigger
rung instead of failing. Every existing assertion is kept, except that the
blended-render test's non-live `is_ok()` becomes "any error must be the
cancellation" — the same TOCTOU reason; no deadline was widened, and "never
live after all attempts" still fails the test.

| Test | Shape shared? | Fixed |
|---|---|---|
| `core/tests/phase8e_coverage.rs` `coverage_cancellation_before_and_during_work_is_atomic_and_releases_scratch` | the diagnosed one | yes |
| `core/tests/precision_layer_blending.rs` `blended_render_cancellation_releases_scratch_without_changing_sources` | yes — `live` sampled, then `cancel()`, live arm asserts `is_err()` | yes |
| `core/tests/refinement_cutout.rs` `live_cancel_during_pixel_copy_never_publishes_and_releases_scratch` | yes — live arm asserts `output == 0` | yes |

The SECOND hole the review named is deliberately left open and is not this
fix: after four (two, in `refinement_cutout`) attempts that never overlap a
live window, `assert!(observed_live, …)` still fails — that is the test's
whole point, and widening it would stop proving mid-work atomicity.

## Numbers

| Measurement | Tolerance, fixed before measuring | Measured |
|---|---|---|
| r1 rotate 30° bilinear vs the drive's own f64 reference | alpha ≤ 1 code, RGB ≤ 1 where the reference alpha ≥ 1 | 1 alpha, 1 RGB |
| r1 same, bicubic | ≤ 1 | 1 alpha, 1 RGB |
| r1 non-vacuity control vs the UNROTATED picture padded into the same canvas | ≥ 32 codes | 255 (both samplers) |
| r3 saved channel vs the scalar reference, 3360 canvas pixels | ≤ 1 | 1 |
| r3 channel control vs the padded channel | ≥ 32 codes | 255 |
| r4 every exact quarter turn vs the drive's own permutation | sample for sample | equal |
| r4 +89.999° `nearest` window (1, 1, 36, 48) vs the 90° render | sample for sample | equal |
| r4 +89.999° vs −89.999° | ≥ 32 codes | 255 |
| r8 `.rz` save → reopen → resave | EXACT | SHA-256 identical |
| r8 recorded `degrees` | a JSON NUMBER equal to 30 | 30.0, number |
| r8 `run_action` replay | sample-for-sample equal | equal |
| 9H sign: +30° CANVAS vs +30° VIEW marker displacement | ≤ 1.0 px | 0.412 px |
| core (c) quarter turns on 64×48 AND 63×48 | bit for bit | equal |
| core (d)/(e)/(f) rasters, every mask kind and channels vs the shared reference | ≤ 1 code of 255 | within |

`render` answers a 16-bit document with its 8-BIT display projection, so
every gate in this run's drive that touches a 16-bit document is a REFUSAL
gate and compares no samples.

## Receipts

`~/devel/rasterize-fixtures/phase12-receipts/12f-1b3/` —
`12f1b3_rotation/receipt.json` (r1–r8), `12f1_geometry/` (A1–A8, b1–b5),
`12f1b_geometry/` (E1–E7), `9h.json` (32 passed groups + pass 2: 1,
`rotate-canvas-sheet-ui` in `required_cases`), `8a.json`, and the ten
12D/12E drive folders.

## 12F-1b4 shipped; remaining open owners

*(Re-cut after 12F-1b3. The 12F-1b bullet was re-owned to 12F-1b2, then to
12F-1b3, and what is LEFT of it after 12F-1b3 shipped the whole 8-bit op is
the TYPED twin — carried below as 12F-1b4, verbatim from the 12F-1b3 spec's
"Out of scope" — plus O1's remainder. P7-28 closes only when all of these
are in.)*

- **12F-1b4 — SHIPPED: typed arbitrary canvas rotation at 16 bits.** *(The
  binding contract below is retained as shipped history. It was carried from
  the 12F-1b3 spec's "Out of scope". 12F-1b3's own criterion — the 8-bit op,
  its core tests, the sheet, MCP, recording, the drive and the probe — is met
  and recorded above; the byte half of 12F-1's bullet, including its
  now-DONE float clause and its now-DONE `canvas_size` phantom-step clause,
  is history and is not carried again.)*

  `GeometryEdit::RotateCanvas { degrees, filter }` (serde tag
  `"rotate_canvas"`, both keys REQUIRED, unknown keys refused) planned from
  the SAME `doc_rotate::{rotation_kind, rotated_canvas}` the byte op reads,
  so both depths share one extent, one matrix and one exact-form decision:
  Identity → `Ok(None)`; Rotate90/180/270 → the existing quarter-turn edit
  bit for bit, PPI transposition and `geometry_*` chrome included; Filtered →
  every `Content::Raster` and EVERY mask `plan::affine(source, m, filter,
  rgba)`, every channel resampled STRAIGHT from the old canvas into
  `(0, 0, W′, H′)` with `m`'s inverse and `outside = 0` (R9), chrome
  `padded_*` by `o`, PPI untransposed, style factor 1, lock-exempt, a
  resampled raster drops its description, a retained source refuses through
  `geometry_plan`'s existing arm and float through `float_gate`'s `_` arm
  (`REFUSED_GEOMETRY` already names it). `precision_geometry_plan.rs` is 586
  of 600 lines after the dispatch MERGES Straighten's and
  RotateCanvas's arms behind one `canvas::` dispatcher, all logic in
  `canvas`. Host: `PrecisionPixelCommand.rotateCanvas`;
  `RasterDocument.rotatingCanvas` routes typed on its FIRST line;
  `DocumentPrecision.rotateCanvasRefusal` and its gate are DELETED in the
  same edit; `rotateArbitrary:` joins the 16-bit `shared` whitelist; family
  `geometry_rotate_canvas`, target `document`, 16 bits only, in core
  `FAMILIES` and `coreRefusal`'s DEFAULT branch. **Accepted when:** multiples
  of 90° equal `rotate90/180/270` bit-for-bit (dimensions, offsets, chrome,
  channels, PPI transposition); a filtered angle matches
  `core/tests/common/inverse_mapping.rs` within 1 code of 65535 with
  premultiplied alpha preserved over loud hidden RGB, for rasters, every mask
  kind and channels; extents equal the byte op's over 12F-1b3's table;
  cross-depth ≤ 1.0 8-bit-equivalent code on `save_copy` output (never
  `render`), control ≥ 32; expanded extents, masks, channels and chrome
  survive Undo and `.rz` reopen → resave byte-identical; the step replays
  through `run_action` at 16 bits;
  `verify_phase12f1b3_rotation.py`'s 16-bit refusal gates become the numeric
  gates; the 9H case gains a 16-bit half committing the sheet equal to the
  numeric op; float keeps refusing by both sentences. Closes 12D's open row
  at 16 bits.

  **Qualification (2026-09-21).** The full release suite, lint, authoritative
  Swift typecheck and signed app build pass. Final isolated receipts are under
  `/private/tmp/rasterize-12f1b4-continuation.RXIbmf`: `rotation/receipt.json`,
  `geometry/receipt.json`, `float-exact/receipt.json` and `9h.json`. Rotation
  r1 measured alpha 0 and RGB 1 code for bilinear and bicubic, with the
  unrotated control 255 codes; r3's saved-channel worst was 0 with a 255-code
  control. The 16-bit saved PNG was genuinely depth 16 and differed from the
  byte twin by at most 0.4980545 byte-equivalent code; its channel difference
  was 0. Every drive reported `ALL GATES PASS`; the UI receipt passed 33
  groups plus the required second process, including
  `rotate-canvas-sheet-ui` and `geometry-noops-ui`. The latter verified zero
  recorded no-op steps and preserved redo at 8, 16 and float; its described
  layer control ran at the two depths that admit text layers.
- **O1's remainder — SHIPPED with 12F-1b4** — a `flip`, or a `rotate` by 180°, whose
  result EQUALS its input. *(O1's argument-decidable half is DONE: 12F-1b3
  made an identity `rotate` and a full-canvas zero-angle `crop` successful
  no-ops at all three depths.)* The five byte exact methods now return
  `Option` through one raw stored-state comparison; typed execution already
  returns `None` for the same result, including float bit equality. Checked
  description/artwork composition continues when editable metadata changes
  despite raw sample symmetry. MCP rotate/flip computes once before undo,
  answers `changed:false` for a complete no-op and suppresses only that call's
  recording result. `geometry-noops-ui` verifies unchanged handle, revision,
  history, dirty state, selection and zero recorder steps at 8, 16 and float,
  followed by a changed exact control that records exactly once.
- **12F-1c — SHIPPED: the interactive drag at depth.** *(AMENDED by 12F-1b2's S2: this
  run no longer owns `.crop` ADMISSION or the Crop COMMIT, both of which
  shipped in 12F-1b2. It KEEPS the REAL-handle drag probe for the Crop box,
  the preview-vs-commit bound, the moved-under-it refusal and all of Move /
  Free Transform.)* `.move` admitted at 16 bits; the Move drag/nudge, Free
  Transform's on-canvas handles (`TransformSession`: affine and ⌘-corner
  perspective) and the Crop box's own handle DRAG.
  Preview and commit share ONE `PrecisionEditorCapture` and ONE
  builder; a preview is a display derivative and never a source — no RGBA8
  transform plate is committed; the commit IS the numeric typed op with the
  gesture's final matrix/quad. **Accepted when:** a 9H probe case drives the
  REAL handles at 16 bits (affine, perspective, Move, Crop + straighten) and
  the committed document equals the numeric op with the same matrix/quad
  sample for sample; an integer Move keeps buffers pointer-equal;
  preview-at-release vs committed display ≤ 1 8-bit code; ONE history step
  per gesture; a gesture whose document moved under it is refused by name;
  recorded steps replay; float per tool migrates (integer Move only) or
  refuses by name; every existing 8-bit probe case stays green, both passes.

  **Qualification (2026-09-21).** `make test`, `make lint`, `make typecheck`
  and `make app` passed. The final isolated UI receipt is
  `/private/tmp/rasterize-12f1c-active.8DIpJ0/9h.json`: all 38
  first-process cases and the second `shortcut-relaunch-ui` process passed;
  the five new cases exercised real Move, Transform and Crop events, native
  sample equality, preview agreement, Undo and Actions replay, stale capture,
  cancellation and float refusals. The lifecycle case recorded the CURRENT
  registered preview ID immediately before each terminal cancellation:
  stale Move, affine, perspective and Crop, Escape, immediate release and
  actual view close each had one active ID; every one later drained to zero,
  along with all older work owned by that gesture. The standalone receipts
  under `/private/tmp/rasterize-12f1c-continue.ZTJQUN` —
  `geometry/receipt.json`, `float-exact/receipt.json` and
  `rotation/receipt.json` — each report `ALL GATES PASS` on unchanged MCP
  geometry paths. Typed `get_document` now includes `offset_x` and
  `offset_y` beside its existing `offset` pair at 16-bit and float depth.
  Group/set affine and perspective handles, wider retained-source geometry
  and filtered float transforms remain assigned to 12F-2 or later.
- **12F-2 — set, group and adjustment-content transforms; retained images.**
  ONE core typed set transform (affine and perspective, all-or-nothing,
  `doc_align::transform_layers`' rules: linked masks ride, unlinked stay)
  replacing the host loop `RasterDocument.precisionTransform`; a group
  transforms every descendant raster and linked mask, an adjustment its mask
  only; linked sets through `movingSet`; multi-layer Free Transform becomes
  RECORDABLE (`transformRecord` "Transform Layers" → a tool + step, closing
  12A's row); retained embedded/linked images (8A) and artwork (8E) under
  whole-document ops (resize, crop, turns, flips, straighten, rotate),
  group/linked-set affine and PERSPECTIVE at both depths, re-rendered from
  the original master — lifting `"rasterize embedded sources before …"` and
  `RetainedImagePolicy`'s `whole` set op by op. Canvas Size is NOT on that
  list: 12F-1 lifted it at 8 and 16 bits on `effective_source`'s lazy
  `translated` fold (G5). Zero-angle Crop is the next candidate by the same
  fold (an integer translation, all `Keep`). A persisted projective binding
  bumps `.rz` under the standing rules (older readers kept, reopen → resave
  identical, 46 first / `LIDS` last) or the rasterization is explicit and
  disclosed. **Accepted when:** set/group results equal the per-entry numeric
  ops composed in the test; exact cases keep codes and allocations; filtered
  cases meet the f64 reference within 1 code with premultiplied alpha; mask
  links and source state survive Undo and reopen at both depths; a retained
  master re-renders within 8A's existing gate (`verify_phase8a.py` green);
  the recorded multi-layer step replays; float migrates or refuses by name
  per op.

  **Implementation and measured receipts (2026-09-21).** Byte and typed
  `affine_set` / `perspective_set` use fixed-point subtree/link expansion and
  one shared canvas map; byte gained bounded plan/edit exports. Retained
  sources carry `SourceTransform::Affine` or `Projective`, with conditional
  native words 51/52 and an `RSPJ` table nested inside identities 45/46.
  Whole resize/crop/turn/flip/straighten/filtered rotate keeps the original
  master at both depths; source info distinguishes projective maps explicitly.
  The host adds `transform_layers`, one recorded Action step, actual group/set
  handles at 8/16 bits and native text/shape regeneration; true perspective
  artwork requires authorization and reports exact description loss. Float
  admits integer set translation but refuses filtered affine/perspective by
  name. The owned MCP drive passed eleven gates, including exact signed-zero
  and HDR float set translation plus named float refusals, projective native
  reopen, retained whole crop/rotate, artwork loss/Undo and replay without
  `__unrecorded`. The full 9H probe passed 40 current groups plus its
  second-process shortcut relaunch; stale source, target and selection,
  Escape, immediate release and view close each drained an active set-gesture
  reservation. Core fixtures now compare nested byte/u16 groups, transitive
  links, masks, noncontiguous/overlapping roots, adjustment entries and
  cancellation per entry; retained grouped/linked masters retain metadata and
  rerender exactly after affine/projective sets at both fallback depths.
  Core `make test`, `make lint`, byte/typed 8A and 8E, and the 12F-1/1b/1b3
  regression drives passed. This is qualified within the declared scope;
  independent review passed with no findings in `phase-12f-2/09-review-3.md`
  and the work was committed as `07d9153`.
- **12F-3 — the initial persisted warp/deform field.** A bounded per-layer
  control lattice evaluated to a displacement field at native depth, RETAINED
  with the pre-warp pixels so it re-edits; a linked mask rides the same
  field, an unlinked one stays, a mask-only warp exists; an identity or
  integer-translation field is an EXACT copy (codes bit-for-bit, allocation
  shared), anything else filtered premultiplied sampling; one conditional
  `.rz` word nested inside 45/46 (older readers refuse by name, reopen →
  resave identical, removing the last field restores the prior bytes); MCP
  tool + Action step + numeric sheet at both depths; float refuses by name.
  **Accepted when:** an f64 field-evaluation + inverse-mapping reference in
  the test within 1 code; the exact-copy cases; independent mask behaviour in
  all three link states; Undo, reopen and re-edit reproduce the same pixels;
  an owned drive.
- **12F-4 — source-coordinate geometry.** 8G `RZLM` local planes remapped
  under geometry-changing decoder migrations (changed orientation/extent
  refuses today) and general remapping: the eight EXIF permutations and
  integer crops/pads keep coverage codes bit-for-bit, anything else filters
  at u16 against an f64 reference within 1 code; separately qualified
  lens-profile distortion (radial model) and transverse CA (per-channel
  radial scale) as source-stage geometry with the local planes riding the
  SAME map, persisted by a recipe version bump that keeps recipes 1–3's
  meaning. **Accepted when:** those numeric gates; a migration that
  previously refused now preserves mask geometry through Undo, Actions and
  native reopen; coverage preserved under distortion within the stated bound;
  11D/13D keep camera metadata, calibration and corpus.

  **Qualification and independent review (2026-09-21):** Final frozen 8G
  all-mode receipt passed 72 host groups, 56 MCP groups and 1,110 calls with
  1,159 exact unchanged inputs and the pinned Nikon D70s source SHA. Eight
  actual-input ICC cases stayed within one u16 code; four genuine Nikon optical
  cases matched at zero u16 codes. Exact EXIF/crop/pad, filtered affine and
  optical references, registered vignette, projective/retouch/independent-mask
  migrations, both fallback depths, 11D formats, Warp, UI/relaunch, catalog,
  typecheck, Release app, lint and formatting gates passed. The full serial Rust
  suite's prior receipt was retained only after checking the final Rust change
  touched comments, leaving all executable Rust statements and tests identical.
  The final independent review reports no findings in
  `phase-12f-4/09-review-3.md`; complete commands are in `08-checks.txt`.
  Explicit profiles do not supply a camera/lens database; broader calibration
  and corpus coverage remain with 13D.
- **12F-5 — byte paint recordability 12A left open.** The byte canvas now
  captures canonical dabs for all six paint/retouch tools whenever it stamps,
  including ordinary mouse soft/flowed/non-round tips, pressure-size/opacity,
  airbrush and size-1 pixel deposits. Quick Mask, layer mask, RGBA plane and
  named channel coverage route through a byte RLE/overlay adapter; clone/heal
  source plates, scalar options and targets are latched. One shared
  `SoftBrush.rasterizeDabs` traversal produces byte commit and replay overlays.
  Historical point calls and native u16 coverage keep their original meaning.
  The 12A inventory identifies the remaining by-design and 12G rows.

  **Measured implementation status (2026-09-22):** The pre-build 12F-4 Release
  executable was pinned at SHA256 `e552d4f489cb92950ddbcaaae70b218dd48d436ff6dd42989f0ee04fa82e77b6`;
  the new owned-app baseline captured seven unversioned byte tool outputs
  before rebuilding. The final owned-app drive matched them, passed seven
  generated canonical Action/replay/Undo families, decoded complete saved
  native planes and metadata, compared all 6,144 selection samples, and
  refused 33 strict cases including binary NaN and generated/canonical
  50,001-dab requests for every tool. The final actual-source UI drive passed
  50 first-process cases plus shortcut relaunch with no changed source inputs.
  Its nine required 12F-5 cases include translated/group/adjustment masks,
  soft selection and ordinary mouse coverage, transformed and varied retouch
  sources/options, exact untouched storage, external-edit cancellation, real
  core/serialization/resource failure rollback, and 14 cap rollbacks. The
  updated 12A replay drive, current 9H numeric drive, eleven unchanged Warp
  gates, Release app and Swift typecheck passed. The full serial Rust suite and
  Clippy receipts remain valid after the final core/header/test hashes matched
  their pinned pre-12F-5 inputs. Exact outcomes are in
  `phase-12f-5/06-checks.txt`, and the fix pass's in `08-checks.txt`. The
  final independent review passes in `phase-12f-5/09-review-3.md` with one nit:
  the spec's `prepareByteStrokeRecord` is served by the existing
  `strokeRecord` builder at commit. No 12G clipboard or 13D camera carryover
  is closed by this work.

### Block B — typed straighten (`crop` with `angle`): CLOSED HISTORY

12F-1 did NOT ship typed straighten and 12F-1b DROPPED it as one unit after
reaching its checkpoint. It was then re-owned to 12F-1b2 and **SHIPPED THERE
ALONE** — see "12F-1b2 — typed straighten" above for what landed, the
decisions S1–S7 and the measured numbers. Its contract, copied verbatim from
the 12F-1 spec and kept here as the record it was judged against:

- **G6 (B) typed straighten is the byte COMPOSITION as one op:**
  `doc_align::straighten_layers(m)` (every raster leaf and EVERY mask, link
  ignored), `transform_channels(m)`, then `crop` — one plan, one undo step.
  The HOST passes the identical `CGAffineTransform` the byte path builds
  (`AgentServer+Retouch.crop`), so the sign convention is shared by
  construction. Chrome is `cropped_*` only (a rotated guide is not a guide —
  the header's rule). Exact forms keep codes through the planner's existing
  `exact_form`. Lock-exempt (`straighten_layers`' reason). Artwork
  descriptions drop in the core; the reply names them as at 8 bits. Float
  refuses through G2's gate — no straighten-specific float code. ~~No UI: the
  Crop tool stays disabled at depth (12F-1c).~~ *(That last sentence is
  SUPERSEDED by 12F-1b2's decision S2: the Crop tool IS admitted at 16 bits,
  display-only until a commit that is the numeric typed op.)*
- **G7 (B) tolerances, fixed now.** Exact cases: bit-for-bit (buffers
  pointer-equal where `Keep`). Filtered vs the test's f64 reference: alpha ≤
  1 code of 65535 everywhere; straight RGB ≤ 1 code where the reference alpha
  code ≥ 1; stored RGBA == 0 where it rounds to 0. Drive cross-depth (16-bit
  straighten of the ×257-promoted picture vs the 8-bit call): ≤ 1.0
  8-bit-equivalent code (16-bit difference ÷ 257) on alpha, and on RGB where
  both alphas ≥ 1 8-bit code; non-vacuity control ≥ 32 8-bit codes against
  the angle-0 crop. Never widened; a miss is recorded with numbers.
- **Step 9 — (B) core** (G6) and `core/tests/precision_straighten.rs`: an f64
  inverse-mapping resampler WRITTEN IN THE TEST (pixel-centre convention and
  transparent out-of-bounds taps per `sampling_walk.rs`; premultiplied
  accumulation; bilinear AND bicubic/Catmull-Rom) under G7 on a fixture with
  a translucent edge and LOUD hidden RGB at alpha 0 (no bleed: that channel ≤
  1 code wherever the reference is 0); exact cases — identity + window equals
  `crop` bit-for-bit, a 90° matrix equals `rotate90` then `crop`, an integer
  translation shares allocations; EVERY mask moves (linked, unlinked, group,
  adjustment); channels ride into the window; chrome equals `cropped_*`;
  style scale; native round trip; retained-source and out-of-window refusals
  by sentence; float `straighten` joins step 2's gate list; identity + full
  window → `None`.
- **Step 10 — (B) Swift:** builder, the typed branch in
  `AgentServer+Retouch.crop` (one `.straighten` command replacing the
  three-call chain at depth; `described` reported, no `withLayerMeta` loop),
  the `DocumentPrecision` gate change, the `crop` catalog sentence; `make
  typecheck`, `make app`.
- **Step 11 — (B) drive gates + doc deltas.** At 16 bits: B1 `crop` angle 7.5
  with `bilinear` and `bicubic` vs the same 8-bit call under G7's cross-depth
  rule + control; B2 a saved channel and guides follow; B3 undo/redo, `.rz`
  identity, a recorded `crop`-with-angle step replays through `run_action`;
  B4 a text layer's `rasterized_layers` note; B5 float refuses `crop` by
  name. Full drive exits 0. Then the (B) sentences: header STRAIGHTEN
  paragraph and ~2476, status doc "what Block B shipped",
  README/`app/CLAUDE.md`/`core/CLAUDE.md`/phase-7 tables, the "straightening
  is unavailable" sweep, and the SECOND `mcp-tools.md` regeneration +
  `--check`.

**Every one of those artefacts is in the tree since 12F-1b2**, together with
S1–S7's additions the 12F-1 contract did not name: the Crop tool's admission
at depth (S2), the shared `RasterDocument.straightenedCrop` both commit sites
call (S3), the `geometry_straighten` family in `coreRefusal` (S5), the
unreadable-`angle` refusal at every depth (S7), the 9H probe case
`straighten-crop-tool-ui` and the drive's b1–b5. The drive's
`--skip-straighten` flag, its `BLOCK_B_CRITERION` and its NOT RUN loop are
DELETED; it runs with no flag and prints `ALL GATES PASS`.

## Out of scope for 12F-1

- Everything in "Open, with owners" above.
- Zero-angle Crop, quarter turns and flips at float (exact ops; a SURFACE
  decision, owner 12F-1b — **shipped there**); Image Size and filtered
  per-layer transforms at float (11F's oracle criterion).
- The 8-bit `canvas_size` same-size phantom undo step: `canvas_resize`
  answered an identical copy there, so the tool succeeded and registered a
  history step where the typed op errored with none. PRE-EXISTING, measured
  by the drive's A4 gate, owner 12F-1b — **fixed there** (decision N1), and
  A4 now expects a successful no-op with 0 steps at all three depths.
- `get_document` previously spelled a layer offset two ways —
  `offset_x`/`offset_y` on byte rows, `offset` as a pair on typed
  (`precision_inspect`) rows. **Fixed in 12F-1c:** typed rows retain `offset`
  and add the byte aliases; the 9H float case asserts both shapes agree.
- P7-38, P7-39's own evaluation and `cmls` (10C-3); `verify_phase8f.py`
  (13D); 12G, 12H.
