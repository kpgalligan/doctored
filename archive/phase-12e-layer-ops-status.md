# Phase 12E — High-depth layer and document operations: status

Slice 12E of [the professional photo editor plan](professional-photo-editor-implementation-plan.md)
is delivered in **three bounded runs**, as the request allows:

- **12E-1 — persisted layer ids and typed structure (reorder, subset
  group/ungroup, delete). Shipped 2026-09-19.**
- **12E-1b — typed merge, typed locks/links, PPI-only, comps by the persisted
  id and `$layer.id` across sessions. Shipped 2026-09-19.** Its seventh item,
  12D-2's carried Shift-equivalence rule, was MEASURED rather than
  implemented, in BOTH event shapes: the leniency 12D-2 inferred belongs to
  hand-built `NSEvent.keyEvent` presses only, and on the shape a keyboard
  delivers a key equivalent matches the chord EXACTLY — so
  `ShortcutMenu.conflict` is right as it stands and the nit is CLOSED
  (below).
- **12E-1c — guides and the ruler origin at depth, with their typed table.
  Shipped 2026-09-19.** It CLOSES P7-26. It also closed 12E-1b's two carried
  comment nits (`merge_down_scratch_bytes`' upper bound, and the View menu's
  Shift sentence, now the measured rule).
- **12E-2 — positional live-adjustment insertion and the continuous typed
  opacity drag (P7-27). Shipped 2026-09-20.** 8A/8E placement and typed
  `new_layer` at depth are SPLIT OUT into 12E-2b, whose acceptance criteria
  are recorded below.
- **12E-2b — 8A/8E placement at a chosen position, typed `new_layer` at
  depth, and the retained-source/RAW lock families. Shipped 2026-09-20.**
  `stamp_visible` / `flatten_image` at depth are RE-OWNED by 12E-2c, whose
  acceptance criterion is recorded below verbatim.
- **12E-2c — `stamp_visible` / `flatten_image` at depth. Shipped 2026-09-20.**
  It ran BEFORE 12E-3.
- **12E-3 — knockout (Shallow/Deep) and `blend_interior_as_group`, the BYTE
  half (P7-37). Shipped 2026-09-20.** Its byte-side DESIGN (D1–D12 below) was
  written into this doc before any code, and the run delivers the byte
  compositor, the f64 oracle, byte `.rz` persistence behind word 49, the named
  typed/float/linear-light refusals and the point-of-action merge disclosure.
- **12E-3b — the typed twin, typed `.rz` word 50, the Blending Options UI and
  EXACT merges. Shipped 2026-09-20.** It delivers the typed compositor (D1–D8
  at 16 bits), typed `.rz` word 50, the Knockout / Blend Interior Effects as
  Group rows, and Merge Down / Merge Layers evaluated against the REAL
  backdrop at both depths with the residual disclosed on the reply. **PSD
  `knko`/`infx`, PSD `cmls`, D5's Background divergence and the 16-bit
  Photoshop re-measurement are 12E-3c's**, whose acceptance criteria are
  recorded verbatim under "Open, with owners". **P7-37 stays OPEN** after
  12E-3b: the byte and typed compositors are verified, the PSD carriage and
  the 16-bit Photoshop measurement are not.
- **12E-3c — PSD `knko`/`infx` carriage at both depths and D5's Background
  divergence as D13. Shipped 2026-09-20.** Its Photoshop half (P16, PD, PX and
  the two contract observations) is re-owned AS ONE BLOCK to **12E-3d** and
  the `cmls` evidence family to **10C-3**, both with criteria recorded
  verbatim under "Open, with owners"; **P7-37 stays OPEN** and P7-38 is a new
  open row.
- **12E-3d — the 12E-3c Photoshop block: P16, PD, PX and the two contract
  observations. Shipped 2026-09-20.** A MEASUREMENT run: the `resave` and
  `merge_down` harness jobs, the 16-bit `knockout` artefacts and the
  `knockout_bottom` family, then the four Photoshop gates. Its eleven
  decisions (F1–F11) were written into this doc and into the drive header
  BEFORE the first Photoshop call; nothing was tuned toward Photoshop
  afterwards and the measurement itself changed no Rust or Swift (F9 was
  never triggered — no measurement exposed a bug against D1–D13 or E1–E10).
  The review pass that followed added ONE thing to the core: the
  exchange-side DISCLOSURE of observation (a)'s clip-base case on PSD import
  and export (P7-39 below), which changes no pixel and no written byte.
  **Ten P16 MATCH, PD
  DIVERGENCE (disclosed) at both depths, eight PX record verdicts and two
  PX-PAD verdicts MATCH, both observations RECORDED — so P7-37 is VERIFIED
  and plan 12E is Shipped.** The numbers are in the "12E-3d" section below.

The split is the honest one. Persisted identity is a `.rz` format change at
both depths with a version bump and a reader/writer contract; typed structure
is a new core module with five operations and a host gate; typed merge needs a
compositor oracle; knockout has no byte implementation at all and needs a
design first. Delivering them together would have meant one of them arriving
untested.

**P7-26 is CLOSED by 12E-1c.** 12E-1 verified its structure half — subset
grouping/ungrouping, reorder and delete at both depths, with native reopen and
recovery. 12E-1b added merge, locks/links and PPI-only edits, plus comps by the
persisted id and `$layer.id` across sessions. 12E-1c adds guides and the ruler
origin at depth, which is everything the row named.

## 12E-1 scope

Two things, and they are independent:

1. **Layer ids are persisted.** The additive outermost `.rz` wrapper 12B
   deferred now exists at both depths, and it is the one table this format
   ALWAYS writes.
2. **Four pure structure operations gain typed twins**, so `reorder_layer`,
   `reorder_layers`, subset `group_layers`, `ungroup_layers` and
   `delete_layer` answer at 16 bits and at 32-bit float instead of being
   refused by name.

Plus the two carried items 12D assigned here: `set_view`'s malformed-argument
refusal and `close_second_view`'s `view` wording.

### The identity contract

| Question | Answer |
| --- | --- |
| Where it lives | `core/src/identity_native.rs`, the `comps_native.rs` shape |
| The wrapper | RZDC **45** (byte) / **46** (typed) as the OUTERMOST version word, before every prior word (43/44 included) |
| The table | `LIDS`, u32 schema = 1, u32 count, then count u64 little-endian ids, after every prior tail (`LCMP` included) |
| The index | Byte: the bottom-first layer index. Typed: the postorder record index — `LLBL`'s and `LCLP`'s convention |
| Conditional? | **No.** The table is ALWAYS written |
| Size | 16 + 8·records, so a 1024-layer document pays 8 KB |
| Uniqueness | Per DOCUMENT. Two files may carry the same ids |
| Collision | The reader raises the process counter past every id it reads (`doc_identity::reserve_layer_ids_through`), so a layer minted after an open never collides with one the file brought |
| Refusals | A bad tag or schema, a count that is not the record count, a zero id, a duplicate id and a trailing word — all BEFORE a single PNG decodes. The WRITER makes the same two structural checks, so it never emits a file its own reader refuses |
| Older files | A file with no 45/46 word reads exactly as before and MINTS fresh ids. The PSD reader mints too |
| Older builds | Refuse 45/46 by name ("unsupported RZDC version") |
| Round trip | Reopen → resave is BYTE-IDENTICAL, because the reader ADOPTS the file's ids rather than minting over them |

**Why always-written, and not conditional like every table beside it.** An id
is a property of EVERY layer, not of a layer that happens to carry a feature.
A conditional table would make identity survive a save only when some
unrelated state was set — a document that merely lost its last colour label
would silently lose its ids with it. So "a document without the table" means
a FILE with no 45/46 word, which is every file an earlier build wrote.

The cost of that decision is that **every `.rz` this build writes has a new
outermost word**, so no file is byte-identical to the 43/44 writer's. That is
the one place 12E-1 breaks the "an unused feature costs nothing" property the
previous six wrappers kept, and it is deliberate.

**Comps bound by INDEX + NAME in 12E-1, and by the PERSISTED id since
12E-1b** (`LCMP` schema 2, below). Actions' `$layer.id` was likewise honoured
only within `ActionIdentity.session` in 12E-1; 12E-1b honours it across
sessions when a second fact corroborates it.

### The typed structure contract

`core/src/precision_structure_ops.rs` holds a **flat postorder record model**
and the five operations over it. Every contract is the BYTE one, verbatim:
`doc_group_ops` owns the move, the grouping and the ungrouping,
`doc_structure` the sibling reorder and the set delete, and those doc comments
are the specification this module implements at high depth.

**Why a flat list.** A byte document already stores its stack flat with a
per-entry `depth`, so every structural op is a splice over one vector. A typed
document stores a TREE, and the same five ops written over that tree would be
five recursive rewrites that agree with the byte five only by inspection. So
`flatten` produces exactly the postorder numbering `PrecisionDocument::layer`
already indexes, the byte splice runs, and `rebuild` rebuilds the tree through
`PrecisionDocumentLayer::group` — the one place the `records`/`depth`
arithmetic lives. Pixels, masks, styles, Blend If, descriptions and retained
sources travel through as the very allocations they were; the drive and the
Rust tests both assert pointer equality afterwards.

| Op | Request | Report |
| --- | --- | --- |
| Structural move | `{"op":"move_layer_to","from","to","depth"}` | — |
| Sibling reorder | `{"op":"arrange_layer","layer","how"}`, `how` = `front`/`forward`/`backward`/`back` | — |
| Subset grouping | `{"op":"group_layers","layers":[..],"name","pass_through"?}` | `{group, cleared_clip, reordered}` |
| Ungrouping | `{"op":"ungroup_layer","layer"}` | `{cleared_clip, landed}` |
| Set delete | `{"op":"remove_layers","layers":[..]}` | — |

All five go through `rz_precision_doc_edit_plan` / `rz_precision_doc_edit`,
take no samples, and answer NULL for an exact no-op. The two grouping reports
ride in the PLAN reply under `"report"`, so a host reads what the op will
change before it applies it.

Identity follows `doc_identity`'s rule: a move, a delete and an ungroup CARRY
every id; grouping mints for the container alone and its members keep theirs.

The four new capability families are admitted at depth 16 AND 32:
`layer_move`, `layer_group`, `layer_remove` on `raster`/`group`/`adjustment`,
and `layer_ungroup` on `group` only — so `ungroup_layers` naming a raster leaf
at depth is refused with the core's own
"high-depth layer_ungroup does not support target raster" sentence and the
host writes no second wording.

### What moved on the Swift side

`RasterDocument.moveLayerTo` / `arrangeLayer` / `removeLayers` /
`removingLayer` / `ungroupLayer` / `groupLayers` route to the typed request on
their first line, the `withLayerName` precedent. Every existing caller — the
panel's multi-row drag through `LayerReorder.reorder`, `AgentServer+Groups`,
`AgentServer+LayerProperties`, `AgentServer+LayerSets`,
`EditorViewController+Arrange`, Delete Layer, and the Action replays of all of
them — therefore works at depth with no typed branch of its own. Typed edits
never route through a byte surrogate.

`PrecisionGrouping.swift` is DELETED: its `groupAll` was the "every top-level
layer exactly once" stopgap, and the generic paths supersede it.
`rz_precision_doc_group_all` and `RasterPrecisionDocument.groupingAll` stay
exported (an export is never removed) but nothing calls them.

The gate (`DocumentPrecision`) gains the four tools in `typedTools`, the four
families in both `coreRefusal` and `floatCoreRefusal`, and — the part that is
easy to miss — the four families in the INDEX-MAPPED branch of each, because a
family that falls through to `targets = ["document"]` is refused by the core
for a reason the user cannot act on. `targetIndices` reads `from` as well, so
`reorder_layer` is asked about the entry it moves rather than the active one.
The layers panel drops its depth guards on delete, group and the drag (typed
"new layer" is still refused — 12E-2 owns it), and the float whitelist gains
Ungroup, Delete and the four Arrange items.

### The two carried view items

- **`set_view` malformed arguments.** `applyViewArguments` now refuses BY NAME
  every present `zoom`, `center`, `fit` (and `sync_pan_zoom`, in
  `AgentServer+Workspace`) that fails to parse, exactly as `rotation` already
  did. A key-presence check alone answered `ok: true` with the view unchanged
  for `{"zoom": "big"}` or a non-object `center`.
- **`view` wording.** `viewArgument(_:)` is the ONE reader of the argument:
  nil when absent, and refused by name ("view must be an integer ordinal —
  call list_views.") when present and not an integer. `close_second_view` has
  its own `view` description, because the shared sentence says "omit it for
  view 1", which is true of `get_view`/`set_view` and false of a required
  argument.

## What 12E-1 verified

Rust (`core/tests/layer_identity_native.rs`, `core/tests/precision_structure_ops.rs`):
the `LIDS` table assembled independently in the test file; round trips at both
depths that keep every id and re-save byte-identically; a file with the
wrapper stripped that mints; a 47 word refused by name; zero, duplicate,
short, long, trailing and bad-tag tables all refused before a PNG decodes; a
byte document carrying an outside-coverage mask, linear-light evaluation, a
label AND a comp at once, and a typed document carrying a style, Blend If,
clipping, an outside mask, a label and a comp, both round-tripping exactly —
which is what catches an intermediate trailing-bytes check that forgot to make
room for `LIDS`; a fresh mint above every read id at both depths; a clone and
the structural ops still carrying ids; the four capability families at 16 and
32; the five typed ops against exact expected structure tables at u16 and
float with pointer-equal pixels and masks, channels untouched, an adjustment
and a described text layer carried through, every refusal named, and a
linear-light document's refusal walk unchanged.

The drive (`scripts/verify_phase12e1_structure.py`, receipts under
`~/devel/rasterize-fixtures/phase12-receipts/12e-1/`) fixes every expectation
before measuring and runs five gates: **S1** ids surviving `save_copy` .rz,
recovery and snapshots at 8, 16 and 32 bits (the 8-bit document carrying a
label, a comp and the linear-light policy); **S2** the five operations at 16
bits and at float against the exact tables the file states, with unchanged
`sample_pixel` readings, ids carried, one history step each and one undo;
**S3** the three refusals by name with the document's path and dirty flag
unmoved; **S4** the `set_view` refusals and the `close_second_view` wording;
**S5** `get_precision` listing the four new tools at both typed depths.

### Decisions taken in this run, and findings

- **A new group on a FLOAT document is an isolated Normal group, not Pass
  Through.** Byte depth and 16 bits both make a Pass Through container
  (the byte op's default), but the 32-bit float mode has no
  Pass Through at all — `float_document`'s admitted set excludes it and the
  core refuses one by name. `RasterDocument.precisionGroupLayers` therefore
  sends `pass_through: !isFloat`, because the only alternative was a refusal
  the user could not act on. The core's `group_layers` edit takes the flag
  explicitly and still defaults it to true, but FORCES it false on a float
  document (review finding 1) — whatever a caller asks for, the core never
  builds a document its own float renderer would refuse.
- **`RasterDocument.isPrecisionNative` had to learn 45/46.** The Swift sniffer
  that decides which reader opens a `.rz` reads the FIRST word, and an
  unknown one is a refusal ("This native document version is not supported by
  this build."). Because 45/46 is now always written, EVERY file this build
  produces hits that switch — so the drive's very first `open_document` failed
  until the two cases were added. Any future outermost wrapper has to touch
  this file as well; it is the one host-side reader of the version word.
- **The writer refuses a duplicate or zero id**, not only the reader. That is
  this codebase's standing rule (the writer never emits a file its own reader
  refuses) and it caught a test fixture — `native_capacity`'s `shared_layers`
  built 1024 layers by cloning ONE `Layer` struct, so every id was the same.
  No real op can produce that (every duplicate mints); the fixture now mints
  too, and still shares the one pixel allocation it exists to share.
- **The drive checks RECOVERY through the checkpoint's own bytes.**
  `recover_document` only offers ABANDONED copies from a previous launch and
  this drive has no crashed instance, so gate S1 opens the `image.rz` that
  `write_recovery` wrote inside the PRIVATE `RZ_RECOVERY_DIR` this run owns —
  which is exactly the file a recovery reads.
- **The blast radius of the version bump was larger than the spec's list.**
  Thirty-nine test binaries asserted the outermost `.rz` word, sliced a tail
  from the end of a file, or spliced bytes onto it; all of them are updated,
  most through one shared helper (`core/tests/common/identity.rs`) that peels
  the identity wrapper so a test whose subject is an EARLIER wrapper keeps its
  own arithmetic unchanged. Two refusal SENTENCES moved and are recorded
  rather than papered over: a byte reader handed a typed file now says "use
  the preserving high-depth document reader" (it sees 46, not 13), and an
  `LCLP` table spliced under a non-32 outer word is named by the identity
  table the reader was expecting there.

## 12E-1b scope

Six things, each with its own contract below:

1. **The three typed MERGES** — Merge Layers, Merge Down and Merge Visible —
   so `merge_down` and `merge_visible` stop being refused by name at depth.
2. **Typed LOCKS and LINKS**, with `doc_lock.rs`'s four bits and
   `doc_align.rs`'s two link ops verbatim, enforced at ONE point.
3. **PPI-only edits at depth**, with every pixel buffer pointer-equal.
4. **Layer comps bound by the PERSISTED id** (`LCMP` schema 2) at BOTH depths.
5. **`$layer.id` honoured across sessions**, when a second fact corroborates
   it.
6. **The AppKit Shift-equivalence rule** carried from 12D-2 — MEASURED in
   both event shapes, and on the hardware-shaped press a key equivalent
   matches the chord EXACTLY, so nothing was changed and the nit is CLOSED.

### The typed merge contract

`core/src/precision_merge.rs`. Every rule is the byte one:
`doc_structure::merge_layers` owns the multi-selection form,
`doc::merging_down` the two-adjacent-entries form, and
`doc_structure::merge_visible` the contribution form.

| Question | Answer |
| --- | --- |
| Requests | `{"op":"merge_layers","layers":[..]}`, `{"op":"merge_down","layer":N}`, `{"op":"merge_visible"}` — through the one typed edit surface, no samples |
| Structural model | `precision_structure_ops`' flat postorder list, the same splice the five structure ops use |
| Pixel model | The members are cloned into a derived document through `rebuilt` and rendered through the ORDINARY typed compositor, so clip runs, blend modes, opacities, masks, Blend If and styles among them bake exactly as they project |
| Result | ONE canvas-sized leaf at (0,0), Normal at opacity 1, with no mask, style, Blend If, description or retained source |
| What travels | `merge_layers` keeps the LOWEST member's name, id, clipped flag, locks, link, label and level; `merge_visible` the bottom-most contributing RASTER leaf's name, id and label; `merge_down` the LOWER record's name, clipped flag, level, locks and link |
| Refusals | Fewer than two members, members across two parents, an out-of-range index, a hidden lowest/lower operand, a lock refusing the DESTINATION (`EditKind::Merge`), and fewer than two contributing raster leaves — each BY NAME |
| Float | ADMITTED. The members' document carries the float policy, so the merge composites through the same float renderer a projection uses and evaluates nothing new |

**Decision: a TYPED merge is CANVAS-BOUNDED.** The result is one canvas-sized
record at (0,0), and pixels a member holds OUTSIDE the canvas are discarded —
where the byte merge keeps the union of its operands' extents. The typed
compositor has no off-canvas accumulator (`PrecisionScene` renders the canvas),
and giving it one is a phase of its own. The difference is stated in the module
doc, in the C header and in the MCP catalog, so no caller meets it by surprise;
a union-extent typed merge and a typed off-canvas compositor are explicitly out
of scope here.

**Two smaller decisions, recorded rather than hidden.** `merge_down` MINTS a
new id for the merged record, because `doc::merging_down` does (`Layer::new`
mints and the byte op sets name/visibility/clipped/depth/locks/link but not
`id`); the typed twin mirrors that verbatim rather than "fixing" a byte
behaviour in a parity run. And `merge_visible`'s two-leaf floor counts
`Content::Raster` records, where the byte predicate counts `LayerKind::Raster`
— which at byte depth includes an adjustment layer, since a byte adjustment IS
a raster with `meta`. A typed adjustment is its own content kind, so the typed
count is the narrower one.

### The typed lock contract

`core/src/precision_lock.rs`. `doc_lock.rs`'s module doc is the specification:
the four bits, the frozen alpha channel and the two pixels that get their
colour back with it, "a transform is a POSITION edit and never a Pixels one",
Lock All as the one spelling that also freezes the mask, the MaskApply and
Merge rules, and the exemption of rename/reorder/opacity/blend/visibility/
style/label/structure.

Two expressions differ from the byte ones, and each is the cheapest exact one:
`lock_block`'s POSITION walk over a group's descendants reads the POSTORDER
numbering directly (a group at index `i` with `records` records owns exactly
`i + 1 - records ..= i`, so nothing is cloned), while the two LINK ops go
through the flat list because they rewrite one field on many records.

The REPORTING shape differs too: a typed edit reaches the host through
`rz_precision_doc_edit`, whose refusals are named strings, so `under_locks`
answers `Err(lock_sentence(..))` — `"high-depth layer {index} is locked
({transparency|pixels|position|all})"` — where the byte twin answers `None`.
The host's own pre-checks still phrase the sentence the user sees
(`LockFlags.refusal` through `lockBlocking`, which now routes to
`rz_precision_doc_lock_block` at depth); the core's is the backstop for a
caller that did not ask.

**The sweep.** Every typed writer is wrapped: `precision_paint`,
`precision_paint_tools`, `precision_retouch`, `precision_scalar_edit` (RGBA →
Pixels, mask → Mask, channel → ungated), `precision_adjustment`,
`selected_adjustment`, `artwork_document`, `precision_geometry` (per-layer
affine/perspective → Position or Mask; the canvas ops ungated, as at byte
depth), `precision_mask_edit` (set/remove/properties → Mask) and
`precision_mask_apply` (→ MaskApply). Three of those CONSULT the lock
(`lock_refusal`) instead of wrapping it, each for a stated reason: a
NEW-LAYER retouch and an artwork DESCRIPTION edit do not write the target's
own buffer, so there is no alpha to freeze and `under_locks`' no-op latch
would throw the edit away; and a mask APPLY must be NAMED rather than
un-applied. `doc_retouch_output` makes the same choice at byte depth.

**DISCLOSED GAP:** the retained-source and RAW edit families
(`precision_retained_source.rs`, `raw_source_edit.rs`) are not lock-gated at
EITHER depth. Owner 12E-2.

### `LLNK`: the one version word INSIDE 46

| Question | Answer |
| --- | --- |
| Where it lives | `core/src/precision_lock_native.rs` |
| The word | RZDC **47**, written INSIDE the identity wrapper: `46 → 47 → 44 → 42 → …` |
| The table | `LLNK`, u32 schema = 1, u32 count = the RECORD count, then per record u32 locks and u32 link, in postorder — after `LCMP` and BEFORE `LIDS` |
| Conditional? | Yes. Written only when some record carries a lock or a link |
| Refusals | A bad tag, a bad schema, a count that is not the record count. Reserved lock bits are MASKED, never refused |
| Byte depth | Needs no table: `Layer::locks` and `Layer::link` have been in the v7 layer record since long before this |
| Round trip | Reopen → resave byte-identical; releasing the last lock and link restores the 12E-1 writer's exact bytes |

**Why INSIDE rather than outside.** 12E-1 fixed two invariants — 46 is the
FIRST word of every typed `.rz` and `LIDS` is the LAST table — and the host's
only reader of the version word (`RasterDocument+PrecisionMetadata.swift`'s
sniffer) together with `tests/common/identity.rs`' identity peel both rest on
them. Wrapping 46 would have broken both for a feature that needs neither, and
would have meant touching thirty-nine test binaries a second time. An older
build still refuses a 47 body word by name, which is the same forward contract
every wrapper carries.

### `LCMP` schema 2: comps by the persisted id

| Question | Answer |
| --- | --- |
| Written by | BOTH depths, always. Schema 1 is READ and never written again |
| Entry | `{"id": <the entry's layer_id>, "name": S, …}` with NO `layer` key |
| The id | VERBATIM — 0 for an entry that never bound, a DANGLING id for one whose layer is gone; both reopen stale |
| Binding | Some record in the FILE carries that id. The NAME is not compared: the id IS the identity, so a reopened comp survives a reorder AND a rename |
| Agreement | The binary schema word and the JSON `schema` are the same number and MUST agree, or the file is refused |
| `has_ids` | A schema-2 payload in a file with NO `LIDS` table is refused ("native layer comps by id require a layer identity table"), so no crafted file binds by coincidence |
| Refusals | A repeated non-zero id within one comp; a `layer` key in a schema-2 payload; an `id` key in a schema-1 one. An id NO record carries is STALE, not refused |
| Re-minting | The reader raises the process counter past every id the payload carries, so a dangling id can never come back on a different layer |

`comps_native::payload` is shared by both writers, so the two depths cannot
word one schema two ways. The byte `.rz` writer's only change in this run is
that shared payload.

### `$layer.id` across sessions

`ActionPlayer.resolve(.layerRef)`, in three steps: (a) SAME session → the id
alone, exactly as 12A shipped it; (b) ANOTHER session → the id, but only when
the entry carrying it ALSO carries the recorded NAME; (c) otherwise the NAME.

**The decision behind (b):** ids are unique per DOCUMENT, not per file set. A
batch's second file may legitimately hold the same id on a completely
different layer, so an uncorroborated id would aim the step at the wrong
picture — which is worse than falling through to the name. Two agreeing facts
are the bar. Re-running an action over the same reopened document is now the
id path, which is the case the persistence was for.

### PPI at depth

`set_resolution` already had a typed implementation
(`precision_metadata_edit.rs`) and `RasterCore.settingResolution` already
routed to it; what was missing was ADMISSION. `set_resolution` joins
`commonTools` and `FloatDocument.tools`, and the Image Size sheet's three
typed-source branches are gone: a typed source behaves exactly like a byte
one, so Resample and the resolution fields are live, the "16-bit Image Size
preserves the source PPI" refusal is deleted, and a resolution-only commit
with Resample off records `.setResolution`.

`core/tests/precision_resolution.rs` asserts every raster's sample buffer is
POINTER-equal before and after, at u16 and at float. A comparison of the
samples themselves would pass on a copy; a pointer comparison passes only when
the allocation was shared.

### The measured Shift rule (carried item 2 from 12D-2) — E HELD, and the first table was an ARTIFACT

12D-2 measured ONE item alone — an item at ⌘R appeared to fire on a ⇧⌘R press
— and INFERRED that "whichever comes first in the walk swallows the other".
This run set out to OVERTURN that inference, on the grounds that the shipped
Undo ⌘Z / Redo ⇧⌘Z, Save ⌘S / Save As ⇧⌘S and Export ⌘E / Merge Down ⇧⌘E pairs
all work. **They do all work, and the inference does not stand.**

The first 12E-1b pass reported the opposite and recorded a shipped ⇧⌘Z/⌘Z
defect. That table was an artifact of the EVENT SHAPE it was measured with,
not of AppKit's rule: the presses were built with
`NSEvent.keyEvent(…)` handed a LOWER-CASE `charactersIgnoringModifiers`
together with Shift in `modifierFlags`, and `charactersIgnoringModifiers`
ignores every modifier EXCEPT Shift, so no keyboard ever delivers that shape.
The probe case `shortcut-shift-rule-ui`
(`scripts/native-paint-probe/workspace.swift`) now measures every press TWICE
— once in that `synthesized` shape and once in the `hardware` shape the window
server itself produces, obtained by translating a `CGEvent` for the same key
code and flags through `NSEvent(cgEvent:)`, which runs the same keyboard-layout
translation a real press runs (`characters` comes back lower case because
Command is held; `charactersIgnoringModifiers` comes back UPPER case):

| # | Layout | Press | synthesized | hardware |
| --- | --- | --- | --- | --- |
| M1 | `Plain` (⌘R) alone | ⇧⌘R | `Plain` fires | **nothing fires** |
| M2 | `Plain` in A, `Shifted` (⇧⌘R) in B | ⇧⌘R | `Plain` | **`Shifted`** |
| M3 | `Shifted` in A, `Plain` in B | ⇧⌘R | `Shifted` | `Shifted` |
| M4a | both in ONE submenu, `Plain` first | ⇧⌘R | `Plain` | **`Shifted`** |
| M4b | both in ONE submenu, `Shifted` first | ⇧⌘R | `Shifted` | `Shifted` |
| M5 | `Plain` in A, `Shifted` in B | ⌘R | `Plain` | `Plain` |
| M6 | `Shifted` (⇧⌘R) ALONE | ⌘R | `Shifted` | **nothing fires** |
| M7 | the app's OWN Edit menu, Undo (key `z`, mask ⌘) and Redo (key `z`, mask ⇧⌘), both targeted so enablement cannot confound | ⇧⌘Z | `Undo` | **`Redo`** |

**The corrected rule: on the event shape a keyboard actually delivers, a menu
key equivalent matches the chord EXACTLY.** A ⇧⌘R press does not fire a ⌘R
item (M1) and a ⌘R press does not fire a ⇧⌘R item (M6); where both exist, the
item whose mask matches fires wherever it sits in the walk (M2 and M4a — the
two the synthesized shape got backwards). The Shift leniency and the
"first-in-the-walk wins" ordering are properties of hand-built
`NSEvent.keyEvent` presses only.

**The expected outcome this run was written against (E: "an exact item always
wins, so the pair is not a conflict") HOLDS on the hardware-shaped event**, and
**there is no shipped defect**: M7 fires Edit ▸ **Redo** on a hardware-shaped
⇧⌘Z, so the shipped ⌘Z/⇧⌘Z, ⌘S/⇧⌘S and ⌘E/⇧⌘E pairs behave exactly as they
appear to. Consequently:

- **`ShortcutMenu.conflict` is UNTOUCHED, and that is now the MEASURED answer
  rather than a deferral.** It compares chords exactly, which is what the
  hardware column shows AppKit doing, so ⌘R and ⇧⌘R are two chords to both of
  them. Its doc comment states the corrected rule and points at this section.
- **The probe case asserts the MEASURED values in BOTH columns**, so it is a
  regression test both for what AppKit does and for the shape that misled the
  first pass.
- **12D-2's carried item 2 is CLOSED by measurement.** Nothing is carried to
  12E-2 from it.

No 12E-1b gate depends on the rule either way.

## 12E-1c scope

One thing, with a contract below: **GUIDES and the RULER ORIGIN at high
depth**, moved by every typed canvas op, persisted in their own conditional
table, and reachable through typed FFI twins so the existing guide UI and the
five MCP tools work at 16 bits and at float without knowing the depth.

### The typed guide contract

`core/src/precision_guide.rs`. **Every rule is `doc_guide.rs`'s**, and the
module states none of its own. The five mutators the byte document has were
HOISTED out of `impl RzDocument` into list-level free functions —
`doc_guide::{added, moved, removed, cleared, origin_set}` — and both depths
are now one-line wrappers over them, so a refusal, the four-decimal
quantization, the duplicate test, the `MAX_GUIDES` cap and the
`[0, extent]` range are each written once. `guide_tests.rs` is the regression
for the byte half, unchanged and still green.

`PrecisionDocument` gains `guides: Vec<Guide>` and
`ruler_origin: (f64, f64)`; `build` makes a document without either and
`rebuilt` carries both verbatim, beside the comps.

The one genuinely typed piece is `geometry_chrome`. The byte depth builds its
moved guides inside each geometry op's own struct literal (`doc.rs`'s
`geometry`, `crop`, `canvas_resize`, `resize`); the typed depth has ONE
canvas-geometry entry point taking a `GeometryEdit`, so the per-op choice is
made once: Resize → `resized_*` with `fx = new_w / width`, Crop →
`cropped_*`, the five turns/flips → `geometry_*` with `doc::Geometry` and
`lw = lh = 0`, a per-layer Affine or Perspective → unchanged. The moved
chrome also counts toward `changed`, so a crop that shifted nothing but a
guide is still an edit.

`Guide` now derives `PartialEq`, which is what that `changed` test compares;
it includes the id, and every helper that carries a guide through keeps it.

### `LGDE`: the second version word INSIDE 46

`core/src/precision_guide_native.rs`, `.rz` version **48**, in the slot
`precision_lock_native` established: `46 → 47 → 48 → 44 → …`, table order
`… → LCMP → LLNK → LGDE → LIDS`. 12E-1's two invariants are untouched — 46 is
still the FIRST word and `LIDS` still the LAST table — so
`RasterDocument+PrecisionMetadata.swift`'s sniffer and
`tests/common/identity.rs`' identity peel needed no change.

Framing: `LGDE`, u32 schema 1, f64 origin x, f64 origin y, u32 count, then
per guide u8 orientation and f64 position — the BYTE version-8 block's layout
after an 8-byte tag/schema prefix, so the two depths store the same nine
bytes per guide. The table is written ONLY when the document carries a guide
or has moved its origin, so a plain document's bytes are exactly the 12E-1b
writer's and clearing the last guide with the origin back at `(0, 0)`
restores them; reopen → resave is byte-identical because the reader adopts
what it reads. The REPAIR/REFUSE split is `rzdc_read`'s: a position is
repaired (non-finite drops that guide, out of canvas clamps, everything
quantizes), while the orientation byte and the count are refused by name
("unsupported native guide schema", "invalid native guide count",
"unsupported native guide orientation"). Guide IDS are deliberately NOT
persisted at either depth, so a reopen mints fresh ones.

### One typed op helper

`ffi_precision_metadata.rs`' private `typed_op` was HOISTED to
`ffi_util::precision_op` and made NULL-tolerant like `doc_op`. It is now the
only `FnOnce(&PrecisionDocument) -> Option<PrecisionDocument>` wrapper in
`core/src`; both metadata callers and all five new guide mutators go through
it. There is still deliberately no capacity gate in it: none of those edits
changes a plane, and guides are bounded by `MAX_GUIDES`.

### What moved on the Swift side

Nothing above `RasterCore.swift`. The ten guide wrappers branch on
`precision` and a private `wrapTyped` boxes the typed handle, so
`Guides.swift`, `EditorViewController+Guides.swift`,
`AgentServer+Guides.swift`, `AgentCatalog.swift` and `ActionPlayer.swift` are
untouched and `ImageDocument.applyGuideEdit` remains the one edit path. The
gates opened: `DocumentPrecision.commonTools` gains the five guide tools, both
whitelists in `EditorViewController+Precision.swift` admit `newGuide:` /
`clearGuides:` and the two guide-preference prefixes, and
`PrecisionModeConversion.losses` no longer claims guides are discarded.
`AgentServer+Precision.swift`'s typed `get_document` now reports the `guides`
and `ruler` blocks through the byte path's own two builders, so the two
depths answer that question the same way.

### Out of scope, recorded

- **`padded_guides` / `padded_origin` have no typed caller** — CLOSED by
  [12F-1](phase-12f-geometry-status.md): the typed `canvas_size` op's
  `geometry_chrome` arm is their caller at both depths, and
  `RasterDocument.canvasResized` routes typed on its first line.
- **Canvas geometry at float** was 11F's policy and stayed unchanged here.
  [12F-1](phase-12f-geometry-status.md) since admitted `canvas_size` at float
  (it reads no sample) and made the refusal of everything else EXPLICIT
  (`precision_geometry_canvas::float_gate`); the exact ops — zero-angle crop,
  quarter turns, flips — wait on their float SURFACE, owner 12F-1b.
- **Comps are still lost by assign/convert-profile at depth** (the
  `precision_color*.rs` constructors build without `comps`) — a pre-existing
  12D-1 gap, owner **12E-2**, deliberately not fixed here. The guides and the
  ruler origin ARE carried by those three sites.

## What 12E-1c verified

- `core/tests/precision_guides.rs`, ten tests: the five typed mutators
  answering every byte refusal and quantizing to four decimals; the whole
  geometry table checked twice, against expectations hand-written in the file
  AND against the same ops on a byte document carrying the same guides; a
  per-layer affine leaving the chrome alone; the `LGDE` table assembled by
  hand and compared with the writer's bytes, sitting immediately before
  `LIDS` and after `LLNK`; reopen → resave byte-identical; clearing restoring
  the plain bytes; the four crafted refusals and an unknown word 49; the
  carriage through promote, downconvert, `float_to_u16` and the profile
  edits, with the plan JSON listing both under `preserved`; and the float
  fixture taking the mutators and round-tripping the table.
- The owned drive `scripts/verify_phase12e1c_guides.py`, gates G1–G5, at a
  tolerance of EXACT (both sides quantize to four decimals): the five tools
  at 16 bits with `list_guides` and `get_document` agreeing and four
  undo/redo steps; crop, rotate, flip and two Image Sizes against formulas
  restated in Python; `.rz` and recovery carriage with a guide-less save
  carrying no `LGDE`; the same at float; and an 8-bit → 16-bit
  `convert_precision` keeping both.
- `make test` (whole suite), `make lint`, `make typecheck` (0 errors),
  `make app`, the 12E-1 and 12E-1b drives re-run, and the 9H UI probe still
  green. Receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12e-1c/`.

## 12E-2 scope

One core rule and one gesture, plus four items 12E-1b carried here.

### The slot rule — ONE rule, both depths

`core/src/layer_insert.rs` is the whole rule and its module doc IS the
specification. An insertion names an ANCHOR and an `InsertPosition`, and the
rule is expressed ONCE over a bottom-first postorder list of LEVELS — the
byte `Layer::depth` and the typed `FlatRecord::depth` — so the two depths
cannot word it two ways:

```text
Above      at = subtree(anchor).end      level = level[anchor]
Top        at = levels.len()             level = 0
IntoGroup  at = anchor                   level = level[anchor] + 1
```

`precision_insert.rs` is the typed expression of that rule over
`precision_structure_ops`' flat list (`flatten` → splice one `FlatRecord` →
`rebuild`), the same model the five structure ops and the three merges
already share, and `ffi_insert.rs` is the two exports
(`rz_doc_adding_layer_at`, `rz_precision_doc_add_adjustment_at`). Nothing
existing was re-signed: `rz_doc_adding_layer`, `rz_doc_adding_image_layer`
and `rz_precision_doc_add_adjustment` are untouched, and
`add_adjustment_at` keeps `add_adjustment`'s admission and job sequence
statement for statement.

**DECISION — a new entry JOINS a clip run rather than splitting it.** When
the NEXT SIBLING at the slot's level is clipped, the new record is created
clipped too. It is the only choice that leaves the neighbours alone: a
clipped run is purely positional at both depths, so an UNCLIPPED record
dropped into the middle of one would become the new base of everything above
it and silently re-base entries the caller never named. The sentence is in
the C header and in the MCP catalog.

**DECISION — insertion is lock-EXEMPT.** It is STRUCTURE: it writes no
existing layer's pixels, alpha or position, so it sits with delete,
duplicate, reorder, group/ungroup and rename on `doc_lock`'s exemption list.

**DECISION — the UI inserts `.above` the ACTIVE entry at every depth.** That
is the byte behaviour plus the clip-run join, so New Adjustment Layer at 16
bits now does what it does at 8; `top` and `into_group` are MCP/Actions
positions with no UI of their own (12E-2b may add one). The recorded UI step
is unchanged (`ActionSteps.addAdjustmentLayer`), and an MCP call records
verbatim, so a positional insert replays.

`add_adjustment_layer` gains `position` (`above` | `top` | `into_group`,
default `above`) and `layer` (the anchor, default the active layer, resolved
by `structuralLayerIndex` so a GROUP is a legal target), and the reply adds
`"position"`. **No gate edit was needed:** the tool stays in family
`adjustment_add` with `targets = ["document"]` in
`DocumentPrecision.coreRefusal` / `floatCoreRefusal` and stays in
`FloatDocument.tools`, so both arguments are admitted at 16 bits and at
float by the gates that were already there. The 11E walk is
position-independent — `capture.publish` already throws
`linearLightRefusalText()` — so no second guard was added.

`PrecisionEditorCapture.requireTopInsertion` is GONE: it was the "New 16-bit
layers currently append at the top" refusal, and with the slot rule there is
nothing left for it to say. The typed create path keeps only the Quick Mask
guard, and the mask write and the `onCommitted` index both read the slot's
LANDING index rather than `layer_count - 1`.

### The continuous typed opacity drag

`PrecisionOpacityDrag` (`app/Sources/PrecisionOpacityDrag.swift`) is the
gesture session, owned by `EditorViewController+OpacityDrag.swift`; the
frozen editor holds one stored property and one closure assignment. ONE
`PrecisionEditorCapture` for the whole gesture, and ONE builder —
`PrecisionOpacityDrag.document(_:opacity:)` — shared by every preview tick
and by the commit, typed all the way down with no `try?` on the publish
path.

**`finish` and `cancel` share a cancel-FIRST prefix** (bump the generation,
`control?.cancel()`, `renderer.cancel()` — which nils `onRender`, so a late
result can never land) and only THEN does `finish` re-check the capture and
build the commit. That order is what keeps the commit's own display
preparation from being refused on top of a preview reservation that is still
live, and it is `PrecisionAdjustmentSession`'s own order. `previewImage =
nil` and the controller's session-nil both sit in `defer`s covering every
exit, the throw included.

**The panel owes the reload.** `LayersPanelViewController.scrubOpacity` is
the one tick body the slider action and the UI probe share. Its typed branch
sets `opacityDragActive`, calls the closure, and on EVERY `ended` tick —
published, a no-op or a refusal alike — clears the flag and THEN reloads,
because `documentDidChange` reduces every notification arriving while the
flag is set to `updateHeaderControls()` and `updateHeaderControls`
suppresses the slider write-back for the same reason. Without it the rows
and the slider would keep showing the abandoned drag value. The slider is
`isContinuous` at EVERY depth now (the `= !doc.isHighDepth` assignment is
gone), and a new read-only `opacitySliderValue` is what lets the probe check
the panel's state.

**Byte staleness is unchanged and is a different mechanism**: a byte gesture
holds `liveEditBase`, so `refuseLiveGestureMutation` refuses a foreign edit
by name and the revision cannot move under it. A typed gesture publishes
whole handles, so staleness is the capture's own contract.

### The four carried items

1. **The Shift-chord rule — closed by 12E-1b's measurement.** Nothing done,
   nothing re-litigated.
2. **`under_locks` on the retained-source/RAW writers — NOT done here.**
   Re-owned by 12E-2b below.
3. **Merge-down identity — DONE, at both depths.** `doc::merging_down` and
   `precision_merge::merge_down` now assign the LOWER entry's `id` and
   `label` over the one their `Layer::new` / `merged_leaf` mints, exactly as
   `merge_layers` and `merge_visible` already did. `doc_identity`'s module
   doc states the rule with NO `merge_down` exception.
4. **Typed adjustment offsets in `moveLayers` — DONE.** The typed branch now
   moves each ADJUSTMENT in the expanded set whose mask is LINKED through
   `performingPrecisionPixelEdit(.affine(layer:, target: .mask, …))`, and
   refuses the whole call first when any adjustment's `lockBlocking(_,
   RZ_EDIT_POSITION)` is non-zero (the byte op's all-or-nothing). A maskless
   or unlinked adjustment stays a no-op, which is the byte behaviour.
   ALL OR NOTHING covers both halves: a refused raster transform and a
   refused mask edit each fail the WHOLE call (`return nil`), so a partial
   move — the rasters left behind while the linked masks moved — cannot be
   published. The probe case `typed-move-all-or-nothing` builds a set with
   exactly one failing member and asserts it, because no MCP call can:
   `AgentServer.moveLayers` pre-checks every position lock by name and caps
   dx/dy at ±1,000,000.
5. **The `precision_guides.rs` NULL-assert nit — DONE.** All five later
   `Box::from_raw` calls assert `!p.is_null()` first, as the first add did.

Beside them, the **comps carriage** the status doc asked for: `next.comps`
is cloned beside `next.guides` at all three profile-edit sites
(`precision_color.rs` once, `precision_color_policy.rs` twice), so a comp
survives an Assign and a Convert still bound.

## What 12E-2 verified

- `core/tests/layer_insert.rs`, seven tests: the three positions against a
  structure table written out in the file; `Above` on the top root agreeing
  with the untouched `adding_layer`; ids carried with exactly one mint;
  `Arc::ptr_eq` on every surviving pixel buffer and mask plane; the input
  document unchanged (purity); an inserted invert against an f64 oracle at
  byte depth, EXACT; an opacity-0 insertion bit-identical over a fixture
  holding a clip run, a group and a styled entry; the clip-run join with an
  oracle that DISCRIMINATES (an unclipped insert would have inverted the
  other half); every refusal by name — the nesting ceiling included, over a
  stack this file BUILDS (a group emptied by a delete and moved into the
  deepest group sits AT the cap, and `into_group` on it lands one level past
  it); and the export's NULL tolerance and bad position code. The TYPED
  ceiling shares that branch but is unreachable by construction — the typed
  limit is a subtree HEIGHT refused when a group is built, so a typed group
  stops one level below the cap — which `precision_insert.rs` records where
  its own refusals are asserted.
- `core/tests/precision_insert.rs`, nine tests: the same table at typed
  depth; `Top` ≡ `add_adjustment` (same table, same carried ids, same
  render); allocations and ids; purity; an f64 invert oracle at u16 within
  ONE code; an opacity-0 insertion EXACT over a fixture holding an isolated
  group, a clip run, a styled record and a Blend If record; the clip-run
  join; under LINEAR LIGHT, `invert` into a group equalling `add_adjustment`
  + `move_layer_to` and `shadows_highlights` refusing with the walk's own
  sentence at all three positions; at 32-bit FLOAT, a +1 EV exposure against
  an f64 oracle at relative 1e-5 and an opacity-0 insertion EXACT over a
  group and a clip run; the refusals; and the export's NULL/bad-code
  behaviour.
- The owned drive `scripts/verify_phase12e2_adjustments.py`, gates A1–A9:
  the three positions at 8, 16 and 32-bit float with the `get_document`
  table, a straight-alpha source-over oracle written in Python (tolerance
  ONE code at each depth, fixed before measuring), one history step each and
  one undo restoring; the clip-run join with every neighbour's identity and
  flag unchanged; the linear-light refusal being the SAME sentence at both
  depths; a recorded positional insert replaying through `run_action` on a
  fresh document; `.rz` save/reopen keeping the position; merge down keeping
  the lower entry's id and label at both depths; and a typed adjustment's
  mask following `move_layers` only while it is LINKED.
- The UI-source probe (`scripts/verify_phase9h_ui.py --require-current`,
  `passed: true`, 24 groups) with three new cases: `opacity-drag-ui` — three
  preview ticks each pixel-equal to a twin document COMMITTED at that
  opacity, exactly one history step at release with one undo restoring, a
  stale gesture ended through the controller that throws and publishes
  nothing, the same stale scenario ended through the PANEL whose sentence
  reaches the presenter and whose slider is left showing the DOCUMENT's
  opacity, and a release mid-preview with no pump between the two ticks
  producing one step, no late preview and `SelectionWorkBudget.scratchBytes
  == 0`; `byte-opacity-drag-ui` — the same three-opacity and one-undo
  checks at 8 bits, with a mid-drag MCP edit refused BY NAME; and
  `typed-move-all-or-nothing` — a 16-bit set holding a raster parked near
  the core's i32 coordinate limit and an adjustment with a LINKED mask, so
  ONE member of the move is refused and the other would move: the set is
  refused WHOLE, the handle it was given is unchanged, and a position-locked
  adjustment in the set is refused the same way.
- `cargo fmt --check`, `make lint`, `make test` (whole suite), `make
  typecheck` (0 errors), `make app`, the 12E-1, 12E-1b and 12E-1c drives
  re-run green, and the public MCP reference regenerated from an owned
  instance on port 4917. Receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12e-2/`.

## 12E-2b scope

Every layer-CREATING path now lands through 12E-2's ONE slot rule
(`core/src/layer_insert.rs`, whose module doc IS the rule), the typed depths
gained a layer-creating op of their own, and the retained-source / attachment
families became lock-gated at both depths.

**Core.** `InsertPosition` derives `serde::Deserialize` (snake_case), so the
JSON spellings `"above"` / `"top"` / `"into_group"` and the header's
`RzInsertPosition` code name the same three slots. `RzDocument::inserting_layer`
is the byte twin of `PrecisionDocument::inserting_record` — set the level and
the clip flag, splice, `doc_group::validated` — and `adding_layer_at` is
expressed over it. (12E-2b also added an `adding_image_layer_at` beside it,
which never gained a caller, an export or a test; **12E-2c DELETED it**, as
that run's carried item asked.) `SourceEdit::Place` gained the
OPTIONAL pair `anchor` / `position`; `parse` refuses one without the other, and
both absent is the legacy push, byte for byte. `Prepared` carries a `slot` at
both depths, resolved ONCE in `prepare_source` and read by the plan's
hypothetical AND the edit, which is what makes the PLAN the positioned edit's
forecast rather than a pushed one. A plan GUARD backs it: after the
hypothetical is built, its record at `prepared.index` must carry a retained
source, else `"invalid source index"`. It never fires on the legacy paths, and
`core/tests/placement_insert.rs` gate (d) was RUN against a deliberately
pushing plan to confirm it discriminates.

`ArtworkPublication` gained `position: Option<InsertPosition>`; `None` is the
pre-12E-2b code path verbatim, so `rz_doc_artwork` and
`rz_precision_doc_artwork` are unchanged, and the two ADDITIONS
`rz_doc_artwork_at` / `rz_precision_doc_artwork_at` take an anchor and a
position code. `StructureEdit::AddLayer` is the typed `new_layer`
(`precision_insert::add_layer_at`): the slot first, then
`preflight_raster_addition`, then u16 zeros or — at float — `from_rgba32_extended`
zeros through `raster_float`, which is `precision_merge::merged_leaf`'s own
storage switch. It joins the `add_layer` op list, the `("layer_add",
&["document"])` family in BOTH capability tables, and `float_document`'s
`ADMITTED`.

**The lock table**, consulted (never wrapped, because `under_locks`' no-op
latch would throw a binding-only change away) at the typed depth:
`set_live_filter` / `capture_raster` / `replace` → `Merge`; `capture_raster`
and `replace` ALSO → `Position` when the destination extent or offset differs
from the record's; `transform` → `Position`; `rasterize` → `Pixels`, AFTER its
no-source `Ok(None)`; `set_link` / `embed` / `place` / `retain` ungated. The
new `RawDocument::attachment_lock_refusal` is `Pixels` at both depths and is
consulted FIRST by `raw_source_edit::plan`, `source_attachment_edit::plan` and
`raw_local_attachment::attach`.

**THE FINDING, recorded rather than repeated.** 12E-1b's disclosure — "the
retained-source/RAW edit families are not lock-gated at EITHER depth" — was
half wrong. The BYTE `doc_retained_source.rs` has consulted `lock_block` since
8A (live filter, capture, replace, transform, rasterize, each with its own
sentence); what it had was no TEST. The real gaps were the TYPED twin, which
had no consult at all, and the RAW / host-media / local-mask ATTACHMENT
writers at both depths. `core/tests/source_locks.rs` now pins every row,
byte sentences included, each asserting the sentence AND an unchanged input.

One consequence is stated rather than discovered later:
`precision_color_policy::assign_profile_rasterizing` calls the typed
`Rasterize` with `?`, so a PIXEL-LOCKED source layer now refuses there by
sentence. That matches the byte twin's lock-included rule and is intended.

**Host.** `RasterDocument.addingLayer(above:)` forwards to the positional twin
and `addingLayer(anchor:position:name:)` routes TYPED on its first line
through `{"op":"add_layer"}` — `precisionRemoveLayers`' shape — so
`EditorViewController.newLayer` (frozen) is untouched and the Layers panel's
`addButton` is simply no longer disabled at depth. `publishingArtwork` gained
`position:` and calls the `_at` exports when it is not replacing;
`nativeArtwork` carries it; `addingDescribedLayer(above:)` passes `.above`, so
the TYPED artwork path lands above the active entry exactly as the byte path
always did instead of appending a top root. The three work enums
(`RetainedImageWork`, `LinkedImageWork`, `HostSourceWork`) take an
`insertion: LayerInsertion?`, thread the pair into the `place` request, and
select/attach at the LANDING index rather than `layerCount - 1`;
`RetainedImageWork.edit`'s `selectNewLayer: Bool` became `landing: Int?`.
`LayerInsertPosition.catalogProperties` / `catalogSentence` are the ONE schema
and sentence the five catalog entries merge in, and
`AgentServer.insertionArguments` is the ONE parser. `add_live_photo_layer`'s
"appends a top root" refusal is deleted: its legacy `layer` IS the anchor now.

## What 12E-2b verified

- `core/tests/placement_insert.rs` — Place at the three positions at BOTH
  depths (structure table, every prior id carried with exactly one mint,
  `Arc::ptr_eq` / sample-address equality on every surviving buffer and mask,
  the input unchanged, the absent-position request identical to `top` on a
  flat stack), the clip-run join, the refusals by name, a positioned `.rz`
  round trip whose reopen → resave is byte-identical, the positioned PLAN
  through `rz_doc_source_plan` / `rz_precision_doc_source_plan` equal to the
  absent-position plan field for field on a 3-entry fixture whose entry 0
  carries NO retained source, and `rz_doc_artwork_at` /
  `rz_precision_doc_artwork_at` at three positions plus NULL and bad-code.
- `core/tests/precision_add_layer.rs` — `add_layer` at u16 and at float:
  transparent, canvas extent, offset (0,0), the projection EXACT (bit for bit)
  at every position, one mint, every prior id carried, the refusals by name,
  and the `layer_add` capability row at both typed depths.
- `core/tests/source_locks.rs` — every lock row above at both depths, the
  ungated four (including a POSITIONED place into a LOCKED group), the live
  filter row through `rz_*_live_filter_edit`, and the RAW and host-media
  attachment writers at both depths, each with its sentence and an unchanged
  input.
- `scripts/verify_phase12e2b_placement.py` — P1/P2 embedded placement at the
  three positions at 8 and 16 bits against a source-over oracle written in the
  drive (tolerance ONE code, fixed before measuring), with neighbour
  identities, one history step and undo/redo; P3 linked placement
  `into_group`; P4 host media `into_group` with `get_host_source` valid AT the
  landing index; P4b `add_live_photo_layer` on an ffmpeg-AUTHORED 4-frame
  movie, positionally and legacy-style; P5 typed `new_layer` at three
  positions at 16 bits and at float with an EXACT composite; P6 a recorded
  positional `new_layer` + `place_image` replayed by `run_action`; P7 typed
  `add_text_layer` / `add_shape_layer` at three positions; P8 a locked
  retained-source layer refusing `transform_layer` (host sentence) and
  `replace_image_source` (core sentence) at both depths with history
  unchanged; P9 the `.rz` round trip; P10 float refusing `place_image` by
  name. `ALL GATES PASS`.
- `scripts/native-paint-probe/newlayer.swift` (`new-layer-panel-ui`) — the
  Layers panel's New Layer button is ENABLED on a 16-bit document, its action
  is still the nil-target `newLayer:`, one invocation makes ONE fully
  transparent record above the active entry at the anchor's own level with
  every neighbour kept, one history step, and one Undo restores; the 8-bit
  document is measured beside it.
- `cargo fmt --check`, `make lint`, `make test` (whole suite), `make
  typecheck` (0 errors), `make app`, the 12E-1 / 12E-1b / 12E-1c / 12E-2
  drives re-run green, `verify_phase9h_ui.py --require-current` passing on
  both passes with `new-layer-panel-ui` present, `verify_phase8e.py` green
  with its rewritten gate, and the public MCP reference regenerated from an
  owned instance on port 4917 and `--check`-clean. Receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12e-2b/`.

**Three things are recorded as MISSES or deviations rather than smoothed
over.**

1. `scripts/verify_phase8f.py` is **NOT RUN**: it needs
   `/tmp/rz-phase8f-fixtures`, built by `scripts/host-source-probe/fixtures.py`
   from the preserved corpus `/tmp/rz-phase8f-feasibility`, and BOTH are
   absent on this machine. No corpus was fabricated. P4, P4b and
   `core/tests/host_source_native.rs` carry the host-media evidence instead.
   **Owner 13D** (scale/corpus qualification), with the criterion in "Open,
   with owners" below and in `designs/phase-8f-host-source-status.md`.
2. `scripts/verify_phase8a.py` failed on the first pass of this run and
   **the gate itself was FIXED in the review pass rather than re-owned**: its
   `native_asset` asserted that the `.rz` version word at BYTE OFFSET 4 is
   14/15, which stopped being true when 12E-1 made 45/46 the outermost word.
   Its INTENT is that the retained asset's native record is carried at its own
   version, so it now WALKS the wrapper chain the way
   `core/tests/common/identity.rs` peels the 45/46 identity wrapper —
   `wrapped_version` steps over each word while it is in the explicit
   `WRAPPER_WORDS` table (25…48, every conditional wrapper since the channel
   metadata one) and asserts the FIRST word that is not a known wrapper is
   14/15, so an unknown word can never be skipped silently. Measured on the
   files this build writes: a byte `.rz` peels `45 → 14`, a typed one
   `46 → 15` (`46 → 17` for a linked source). The gate PASSES: "PASS: 8-bit
   external MCP embedded-source workflow (6 groups)" and the 16-bit twin.
3. P4's HEIC is authored with ImageMagick rather than `sips`. A `sips`-made
   HEIC decodes to a CGImage layout `HostStillDecoder` does not admit
   ("unsupported sample layout"), and widening that decoder is not this run's
   business; ImageMagick is 8F's own encoder for the same fixture family.
   Separately, the drive runs ONE OWNED INSTANCE PER GATE, because the first
   pass deadlocked at P4b with 64 threads parked in
   `-[NSAnimation _runBlocking]` on the user-interactive root queue and the
   main thread stuck in `BackgroundWork.wait`. **The review pass found the
   cause and FIXED it, in one line**: a titled `NSPanel`'s DEFAULT appearance
   animation is a THREADED `NSAnimation`, so every `JobProgressController`
   presentation parked one such thread for the life of the process;
   `JobProgressController.init` now sets `window.animationBehavior = .none`
   before ordering the panel in, and the panel — which appears and disappears
   in place — has nothing to animate. MEASURED with `sample` against one owned
   instance, not assumed: BEFORE, `new_layer` ×6 and `set_layer_properties`
   ×6 park nothing while `place_image` ×6 parks 4 on the first placement and
   exactly 3 on each later one (one per panel presentation; the recorded
   series is 2 → 6, 9, 12, 15, 18, 21) — and 80 consecutive placements reach the
   64-thread soft limit by the 20th; AFTER, the same attribution run is flat
   at 2 through every call, and 200 consecutive worker-backed edits in ONE
   instance stay flat at 2 with the soft limit never reached. Receipts:
   `~/devel/rasterize-fixtures/phase12-receipts/12e-2b/fix-20260920-034944/panel-threads/`.
   The per-gate relaunch is deliberately LEFT as it is in this pass (the drive
   was not re-shaped after the fix); collapsing it back to a single instance
   is the "Open, with owners" item below.

## 12E-2c scope

Stamp Visible and Flatten Image answer at 16 bits and at 32-bit float, through
the existing `rz_precision_doc_edit` structure-edit surface: NO new export, NO
`.rz` version change, and the byte paths untouched byte for byte
(`git diff --quiet HEAD -- core/src/doc.rs core/src/doc_structure.rs
core/src/ffi_doc.rs core/src/ffi_group.rs` succeeds). The eight 12E-1b menu
selectors are whitelisted at both typed depths alongside this run's two, and
`RzDocument::adding_image_layer_at` — 12E-2b's carried dead public API — is
DELETED.

**Core.** `precision_merge.rs` gains `stamp_visible` and `flatten_image` beside
the three merges, in the same `impl PrecisionDocument`, reached as
`StructureEdit::StampVisible { name, anchor }` and `StructureEdit::Flatten {}`.
Both take their pixels from the document's OWN projection — the same
`render_with_context` + `merged_leaf` path `merge_visible` uses, charged as
`merge_scratch_bytes(&[])`. Every rule is the byte one, and the nine that
decide what these ops MEAN are written once in the module doc, once in the C
header comment and once in the MCP catalog:

1. **A stamp does NOT join a clip run.** The byte `adding_image_layer` sets
   only the level and leaves `clipped` false, so the typed op resolves the
   ordinary `Above` slot and then FORCES `clipped = false` before splicing.
   This is the ONE place `layer_insert`'s join-the-run rule is deliberately
   overridden, and `core/tests/precision_stamp_flatten.rs` asserts both halves
   — the slot itself still says JOIN, and `add_layer` at the same anchor still
   joins.
2. **A flatten DROPS the whole comp list.** The byte flatten builds on
   `..from_pixels`, whose `comps` is empty, while `rebuilt` COPIES the comps
   (a rebuild is the SAME document with changed records). So `comps.clear()`
   after `rebuilt` is required, not incidental. The 12E-2b criterion said
   "comps stale by name"; the byte op is stricter than that and the byte op
   wins, so the typed op empties the list.
3. **A flatten's one record is named `"Background"`** — `from_pixels`' own
   name — with a newly minted id, no label, no locks, no link, visible,
   opacity 1, Normal, unclipped, at level 0 and offset (0, 0), and no mask,
   style, Blend If, description or retained source. A stamp's record is that
   same shape with the CALLER's name (the host default is `"Stamp"`), at the
   ANCHOR's level directly above the anchor's whole subtree.
4. **Both ops are lock-EXEMPT, at both depths.** Neither byte op consults
   `lock_block`: a stamp ADDS a record and writes none (`layer_insert`'s
   exemption), and a flatten replaces the whole stack rather than editing an
   entry. No `lock_block` call was added; a Lock All anchor still stamps and
   an all-locked stack still flattens.
5. **A flatten never answers a no-op.** A one-record document still answers a
   new document, because the flattened picture is a new one. Only the MENU
   asks for more than one layer (`EditorViewController.swift`'s frozen
   `flattenImage:` arm, unchanged).
6. **A flatten keeps** the saved channels, the guides, the ruler origin, the
   profile, the metadata, the resolution and the evaluation policy — all
   through `rebuilt`, with the import warnings riding along. Hidden records are
   discarded; the selection, Quick Mask and active-layer handling are host
   state and unchanged.
7. **A stamp's floor is ONE contributing RASTER record**, `merge_visible`'s
   own `contributing` predicate narrowed to `Content::Raster`. That is 12E-1b's
   disclosed narrowing again — the byte predicate counts `LayerKind::Raster`,
   which at byte depth INCLUDES an adjustment layer — so the typed op refuses
   BY NAME ("high-depth stamp visible requires a contributing raster layer")
   where the byte op answers `None`; both reach the host as nil and the
   existing "there is nothing visible to stamp" reply.
8. **The pixel source is the document's own projection**, not a derived
   members document.
9. **There is NO canvas-bounded difference from 8-bit here, unlike the
   merges.** The byte `flattened()` renders the CANVAS, so the byte stamp and
   the byte flatten are already one canvas-sized picture at (0, 0).

`Flatten {}` is an EMPTY STRUCT variant, not a unit one: an internally tagged
serde unit variant silently discards extra keys despite `deny_unknown_fields`
(`raw_local_coverage.rs`'s rule). **By owner decision this run also closes the
same defect in the pre-existing `MergeVisible` variant** — it is `MergeVisible
{}` now, every match arm takes the empty braces, and the Rust test asserts the
unknown-field refusal on all three ops by name. No other variant of
`StructureEdit` was a unit variant.

`layer_stamp` and `layer_flatten` are admitted for target `document` in BOTH
`precision_capability::FAMILIES` and `FLOAT_FAMILIES`, and
`float_document::ADMITTED` gains both names — nothing new is evaluated at
float, because the ops render the same projection the display already does.

**Swift.** `RasterCore.flattening()` and `stampVisible(above:name:)` route
typed on their FIRST line (`mergeVisible()`'s shape), over
`precisionFlatten()` / `precisionStampVisible(above:name:)` in
`RasterDocument+PrecisionOperations.swift` — so
`EditorViewController+MergeStamp`'s handlers, the frozen
`EditorViewController.flattenImage`, `AgentServer+Groups.stampVisible`,
`AgentServer.flattenImage` and every Action replay work at depth with no
depth branch of their own. `stamp_visible` / `flatten_image` join
`DocumentPrecision.typedTools`, both `families` maps (the DEFAULT `["document"]`
branch, not the index-mapped list) and `FloatDocument.tools`.

**The 12E-1b menu whitelist gap, FOUND AND CLOSED here.** The eight selectors
`mergeDown:`, `mergeVisible:`, `linkLayers:`, `unlinkLayers:`,
`lockTransparency:`, `lockPixels:`, `lockPosition:` and `lockAll:` have had
working typed cores since 12E-1b and working first-line typed routes in
`RasterCore`, but were still disabled by `validatePrecisionItem`'s closed
whitelists — a 12E-1b omission, not a policy. All eight plus this run's
`stampVisible:` / `flattenImage:` are now in BOTH the `float` and the `shared`
sets, after a per-selector check of the float route (`layer_merge`,
`layer_merge_visible`, `layer_lock`, `layer_link` are in float `ADMITTED`; no
handler tests `isFloat`; typed `merge_down` / `merge_layers` carry no floor
`EditorViewController.swift:3195-3204` and `canMergeSelectedLayers` do not
already mirror). None was dropped.

Whitelisting `mergeVisible:` would have enabled an item whose TYPED floor is
narrower than `canMergeVisible` read (rule 7: an adjustment leaf contributes
but is not a raster), so the two `can*` properties became DEPTH-AWARE rather
than the beep being disclosed: `RasterDocument.contributingRasterLeaves()` in
`LayerTree.swift` is `contributingLeaves()` minus `layerIsAdjustment`, and
`EditorViewController+MergeStamp`'s `canMergeVisible` (≥ 2) and
`canStampVisible` (≥ 1) read it at depth. The 8-bit answers of both are
unchanged, the frozen controller was not touched, and the floor lives in
exactly those two files.

## What 12E-2c verified

- `core/tests/precision_stamp_flatten.rs` (NEW, 12 tests) — tolerances stated
  in the file header BEFORE anything was measured: **1 code of 65535** at 16
  bits, **relative 1e-5** (scale `max(|want|, 1)`) at float, and **bit
  identity** between the stamped leaf and the flattened leaf. The fixture per
  depth holds a group, a clipped run, a masked layer, a hidden layer, an
  adjustment (invert at u16, +1 EV exposure at float) and a Multiply layer,
  and the projection is computed in this file from an f64 W3C
  source-over/blend equation plus the clip-unit and isolated-group rules
  restated there, with Multiply itself from the independent
  `tests/common/native_blend_oracle.rs`. Claims: both leaves inside tolerance
  and bit-identical to each other at u16 AND at float; after a stamp every
  pre-existing record's sample AND mask ADDRESS equal, every prior
  id/label/lock/link carried, exactly one mint; the leaf unclipped between a
  clip base and its clipped member while `add_layer` at the same anchor still
  joins; landing index and level for a leaf, a group and a nested anchor;
  stamp lock-exempt on a Lock All anchor and on a locked group; flatten
  leaving one `"Background"` with a new id, label 0, locks 0, link 0, EMPTY
  comps and the guides / ruler origin / channels / evaluation kept, a
  one-record flatten still answering a document, and an all-locked stack
  flattening; refusals by SENTENCE — a bad anchor, no contributing raster
  (including a visible-adjustment-only stack, at u16 AND — since the
  review-fix pass — on an all-hidden FLOAT fixture), a missing `name`, a
  missing `anchor`, and an UNKNOWN field on `stamp_visible`, on `flatten` and on
  `merge_visible`, each carrying the `invalid high-depth layer property`
  prefix and leaving no document; the plan charging at least Merge Visible's
  `scratch_bytes` and taking no samples; 11E parity — a `linear_light` u16
  document given a Hue `layer_blend` AFTER the switch refuses BOTH ops, plan
  and edit, with exactly `linear_light_refusal()`'s sentence, while the
  migrated stack answers both; the `layer_stamp` / `layer_flatten` capability
  rows supported for `document` and unsupported for `raster` at 16 and 32;
  and a native round trip (reopen → resave byte-identical) for a stamped and
  a flattened document at u16 and at float.
- `scripts/verify_phase12e2c_stamp_flatten.py` (NEW, ONE owned instance for
  every gate) — C1/C2 Stamp and Flatten at 8, 16 and 32-bit float with the
  tables this file states, the composite unchanged within the tolerances fixed
  in its header (8-bit `render` PNG SHA-256 EQUAL; 16-bit `magick compare
  -metric PAE` of the PNG16 exports ≤ 1 code of 65535; float `sample_pixel`
  at four fixed probes within relative 1e-5), ONE `undo` restoring and `redo`
  reapplying. The fixture's bottom layer is fully OPAQUE on purpose — the
  stamped layer IS the projection, so laying it over the unchanged stack is an
  identity only when the projection is opaque — and the float fixture is
  duplicated 30 times so its composite alpha is exactly 1.0f, ASSERTED at
  every probe rather than assumed. C3 an all-hidden stack answering
  `changed: false` with the history cursor unmoved at 8 bits, 16 bits AND
  32-bit float (the float case added in the review-fix pass, beside the float
  arm of `precision_stamp_flatten.rs`'s refusal test); C4
  a flatten emptying `list_layer_comps` (one `undo` bringing the comp back)
  while the guides and a saved channel survive, at 8 and 16; C5 a recorded
  `stamp_visible` + `flatten_image` pair replaying through `run_action` on a
  fresh 16-bit document; C6 `.rz` save → reopen → resave SHA-256-identical for
  a stamped and a flattened document at 16 bits and at float; C7
  `get_precision` listing both tools at 16 and at float; C8 the FLOAT
  whitelist's MCP proxy — `set_layer_lock` (the document reporting `locks`),
  `link_layers` (a non-zero `link`), `unlink_layers`, `merge_down` and
  `merge_visible` each answering non-error with one `undo` restoring.
  `ALL GATES PASS`.
- `scripts/native-paint-probe/stampflatten.swift`
  (`stamp-flatten-menu-ui`, registered in `main.swift`'s non-second-pass
  cases) — (a) on a 16-bit document with TWO contributing raster leaves,
  `validateUserInterfaceItem` answers TRUE for all ten selectors
  (`unlinkLayers:` asked after a `linkLayers:` invocation, which is measured
  to have produced a non-zero link), the `mergeDown:` item's title reads
  "Merge Layers" multi-selected and "Merge Down" single-selected, `lockPixels:`
  goes `.off` → invoke → `.on` → Undo → `.off` in ONE history step, and Stamp,
  Merge Visible and Flatten are each ONE history step one Undo restores with
  Stamp selecting the landing index; (b) on a 16-bit raster + invert
  adjustment, `mergeVisible:` is FALSE while `stampVisible:` and
  `flattenImage:` are TRUE, and hiding the raster turns `stampVisible:` FALSE;
  (c) the SAME 8-bit stack answers TRUE to both, so "byte is unchanged" is an
  assertion.
- `cargo fmt --check`, `make lint`, `cargo test --release --test
  precision_stamp_flatten`, `make test` (whole suite), `make typecheck` (0
  errors), `make app`, the 12E-1 / 12E-1b / 12E-1c / 12E-2 / 12E-2b drives
  re-run green, `verify_phase9h_ui.py --require-current` passing on both
  passes with `stamp-flatten-menu-ui` present, and the public MCP reference
  regenerated from an owned instance on port 4917 and `--check`-clean.
  Receipts under `~/devel/rasterize-fixtures/phase12-receipts/12e-2c/`.

**Recorded as misses or deviations rather than smoothed over.**

1. `scripts/verify_phase8f.py` is **NOT RUN** again, for the same reason:
   its corpus is absent from this machine. **Owner 13D**, criterion below.
2. The FLOAT selector whitelist has **no UI-source gate**: the 9H probe has
   no float opener, so `stamp-flatten-menu-ui` asserts (a) and (b) at 16 bits
   only and says so in its own report. The drive's C8 float MCP proxy is the
   evidence in the meantime. **Owner 12E-3**, criterion below.
3. `make test`'s whole-suite runs reported
   `phase8e_coverage::coverage_cancellation_before_and_during_work_is_atomic_and_releases_scratch`
   failing with "fixture did not overlap live coverage work" — one of the
   known under-load timing flakes (it passed 18/18 in isolation). **FIXED in
   the review-fix pass, in TEST code only**: the three tests of that
   live-cancel shape — that one, plus
   `blended_render_cancellation_releases_scratch_without_changing_sources`
   (`core/tests/precision_layer_blending.rs`) and
   `live_cancel_during_pixel_copy_never_publishes_and_releases_scratch`
   (`core/tests/refinement_cutout.rs`) — no longer ASSERT the race. Each polls for
   the live window, branches on its outcome, and RETRIES the whole scenario
   on a larger workload (up to four/four/three attempts) until the observer
   genuinely catches the worker mid-work; the atomicity and scratch-release
   assertions are unchanged and still run in that arm, and never observing a
   live window in ANY attempt still FAILS the test, so mid-work cancellation
   is proven on every run rather than skipped under load. No deadline was
   widened and no assertion was deleted. Both whole-suite `make test` runs
   after the change exit 0 (210 binaries, all `ok`).

## 12E-3 scope

ONE thing, at 8 bits only: per-layer **Knockout** (None / Shallow / Deep)
and **Blend Interior Effects as Group**, as two new keys of the layer
style's BLENDING OPTIONS — designed here first, then implemented in the byte
compositor, qualified against an independent f64 oracle and against
Photoshop-rendered fixtures, persisted in byte `.rz` behind a conditional
version word, and refused BY NAME at 16 bits, at float and under
`linear_light`. No new FFI export.

### The byte design

Photoshop (26.10.0) was found installed at `/Applications/Adobe Photoshop 2025/`
and not running. Notation, straight RGBA: `over(B, C, a, M)` = W3C source-over
of colour `C` at alpha `a` onto `B` with blend `M`; `mix(A, K, t)` =
premultiplied lerp (`style_composite::mix_premultiplied`).

- **D1 Home.** Both are BLENDING OPTIONS in the style JSON beside
  `fill_opacity`/`blend_if`: `"knockout": "none"|"shallow"|"deep"` (default
  none), `"blend_interior_as_group": bool` (default false). Consequences,
  deliberate: every style surface carries them with no new export
  (`rz_doc_set_layer_style`, `get_document.style`, Copy/Paste Layer Style, the
  recorded `set_layer_style` step, comps' appearance); an adjustment layer
  refuses (existing rule); a knockout GROUP is a unit because a style isolates
  it (`doc_group::is_isolated`); 11E refuses through its existing
  `"a layer style or Blend If"` walk and 11F through its existing style
  refusal — no new policy row. MCP surface is `set_layer_style` ONLY (it takes
  the WHOLE style, so a caller adding a key must send the layer's existing
  style with the key merged in); the request's `set_layer_blending` does not
  exist and is not added.
- **D2 Identity.** `knockout != none` makes a style non-identity.
  `blend_interior_as_group` alone does NOT (it renders nothing without a
  grouped effect), so a lone flag clears.
- **D3 Order** (a non-Pass-Through group composites first in isolation;
  interior effects follow the pixels unless grouped). For a styled
  unit: (1) Blend If snapshot — Underlying reads the ACCUMULATED composite
  beneath the layer, never the knockout backdrop; (2) Below effects onto the
  accumulator, untouched by knockout; (3) the INTERIOR UNIT evaluated against
  the knockout backdrop `B0` (D4) and mixed in by the SHAPE; (4) the remaining
  Interior effects onto the accumulator as today; layer opacity stays the
  package. Per pixel: `s = alpha·mask`, `q = fill·w` (`w` = Blend If weight —
  Blend If is OPACITY, never shape: a pixel it hides still punches),
  `K = over(B0, C, q, M)`, `out = mix(acc, K, s·opacity)`. With `B0 = acc` this
  IS today's pixel statement (`sa = s·q·opacity`); with `q = 1`, Normal it
  equals the plain render for ANY `B0`, so soft edges get no fringe. A clip
  BASE with visible members uses today's unit buffer `U` (pixels at fill,
  members clamped to the footprint `s`): `K = over(B0, U.rgb, min(1, U.a/s), M)`,
  same mix. Dissolve tosses on `q`.
- **D4 Boundary rule.** `B0` is a buffer ALIGNED with the accumulator in play,
  or transparent. Inside an ISOLATED group's buffer, a merge/bake/
  `rendered_group`/per-item accumulator: BOTH modes stop at that buffer's
  initial state — transparent, so the hole shows what lies beneath the group
  once it composites. An isolated buffer is a HARD STOP for Deep too. A
  CLIPPED MEMBER, both modes: the clip buffer after the BASE alone rendered —
  and this holds for EVERY member loop, of which there are three: the
  unstyled base (`doc.rs::composite_clip_group_into`), the STYLED base
  (`style_composite.rs::composite_direct_into`'s member loop, ~368, which is
  also what each package strip runs), and a knockout/grouped base's own unit
  pass (`style_knockout::composite_interior_unit_into`). In all three the
  members composite with `env.knockout` REPLACED by that unit's base-only
  snapshot (the unit buffer right after the base's pixel pass, before any
  member; strip-aligned inside a package strip because it is cloned from the
  strip's own buffer) — never the enclosing backdrop. Shallow in a PASS-THROUGH
  group (plain or gated): the accumulator as it stood when the group's children
  began. Shallow at the ROOT ≡ Deep. Deep through any chain of
  pass-through groups: the BACKGROUND (D5), else transparent.
- **D5 Background rule** (Rasterize has no Background layer kind). Entry 0
  qualifies iff: depth 0, Raster and not an adjustment, visible, not cut,
  Normal, opacity 1, no enabled mask, no style, offset (0,0), pixel size ==
  canvas, and entry 1 (if any) unclipped. `B0` = the root accumulator right
  after entry 0 composited. Disclosed divergence: Photoshop tells a true
  Background from an ordinary opaque bottom layer and Rasterize cannot — owner
  12E-3c (re-owned from 12E-3b when that run was cut; the PSD record is where
  a Background is recognizable).
- **D6 `blend_interior_as_group`** (the layer's blend mode applies to Inner
  Glow, Satin, Color/Gradient Overlay). ON moves exactly the four GROUPED
  contributions — gradient overlay, colour overlay, satin, inner glow, already
  the first four of the Interior order — INTO the unit: pixels into `U` at fill
  1, each grouped contribution composited into `U` with its own blend and
  opacity and `U`'s alpha restored after each (the clip-member rule: they
  recolour, never add coverage), then the unit meets `B0` (or `acc` without
  knockout) ONCE with the layer's mode at `q = fill·min(1, U.a/s)`. Inner
  shadow, inside/centre stroke and bevel stay after the unit. Narrowing: on a
  clip base WITH visible members the flag is kept but evaluates OFF —
  disclosed, owner 12E-3b.
- **D7 Merges.** Flatten, Stamp Visible and **Merge Visible** render the
  projection, so knockout bakes EXACTLY in all three: `doc_structure::
  merge_visible` bakes `self.flattened()` — the document's own accumulator,
  the one that carries the Background snapshot — so it is not narrowed at all.
  (This is the run's one CORRECTION to the design as first written, made after
  reading the code and pinned by `knockout_composite.rs`; the first pass
  narrowed Merge Visible too, which was a false disclosure.) Merge Down, Merge
  Layers and the PSD export bake composite into private accumulators
  (`doc.rs::merging_down`, `doc_group_ops.rs`), so `B0` is transparent and a
  hole bakes as TRANSPARENCY: the merged layer then shows whatever lies beneath
  IT rather than the knockout's backdrop — a real picture change whenever
  anything sits between. Disclosed AT THE POINT OF ACTION, the PSD export loss
  sentence's pattern: the `merge_down` MCP reply (both the single and the
  `layers` branch) carries a `note` (Interfaces, exact sentence) whenever a
  visible entry in the merge set — members and their subtrees — has
  `knockout != none`, computed BEFORE the edit because the merge drops the
  style; that tool's catalog text says the same. `merge_visible` carries
  NEITHER a note nor a narrowing sentence; its catalog text states that it
  bakes knockout exactly. Also in the module doc, the header and the status
  doc. The MENU items have no message surface and gain none; no UI control can
  SET knockout this run (12E-3b adds the control and, in the same run, makes
  Merge Down and Merge Layers exact — contract (g) — so the UI never ships a
  settable knockout with this narrowing in place).
- **D8 Cost.** One accumulator-sized f32 snapshot for the Background (only when
  a visible style has knockout ≠ none and D5 holds), one per pass-through level
  holding a visible Shallow child (a gated pass borrows the untouched
  accumulator instead), one per clip unit with a visible knockout member
  (unstyled, styled or unit-pass base alike; strip-sized inside a package
  strip); each charged to `doc_group::RenderBudget` and counted by
  `render_scratch::projection_scratch_bytes`. A document with neither key
  allocates nothing and projects byte-identically.
- **D9 Writer.** `to_json` emits each key ONLY when non-default, so every
  stored style, `.rz`, `LSTY` body, comps payload and PSD golden without them
  is byte-identical. Strict parse: wrong type or unknown `knockout` string is
  an error naming the key; lenient: unknown string → `none`.
- **D10 Byte `.rz`.** Conditional version word **49**, NO table, nested INSIDE
  45 (`45 → 49 → 43 → 41 → 38 → 28 → body`), written iff some layer's style
  carries a non-default key. The reader refuses 49 with no such style
  (`"invalid native knockout word"`) and such a style without 49 (`"native
  knockout styles require RZDC 49"`), both before any PNG decodes; older builds
  refuse 49 by name (`unsupported RZDC version 49`; they would otherwise drop
  the keys and clear an "identity" style). `RZDC_VERSION` stays 46; the first
  word stays 45, so the host sniffer is unchanged. Typed word 50 inside 46 is
  12E-3b's.
- **D11 PSD carriage DECISION: read AND write `knko` and `infx` at both depths;
  `clbl` stays observation-only** — delivered by 12E-3c (re-owned from 12E-3b
  when that run was cut), because the `knko`
  byte's Shallow/Deep values are not in Adobe's table and must be READ from a
  Photoshop-authored file first; this run records them (`blocks.json`). Until
  then import keeps `psd_feature_markers`' existing disclosed `knko` loss and
  export adds a per-layer loss sentence; the committed goldens stay
  byte-identical.
- **D12 Oracle, tolerances and the comparison's status — FIXED NOW.** Oracle =
  D3–D6 in f64 over hard-edged and one partial-alpha scene. **T1** byte render
  vs oracle: 1 code of 255 per channel, alpha included. **T2** Photoshop 8-bit:
  max RGB ≤ 2 codes per case; the Blend If case ≤ 4 (the 10C A4 precedent);
  transparent results matted on white first (Photoshop's PNG export does).
  **T3** (12E-3b) typed vs oracle 1 code of 65535; vs Photoshop 16-bit ≤ 2
  8-bit-equivalent codes. Each P-case has exactly one of three statuses.
  **MATCH**: the gate passed, render within T2. **MISS**: the gate passed,
  render over T2 — recorded with its max/mean, never widened, keeps P7-37 open
  with that case as its criterion (the 11C precedent). **NOT RUN**: the GATE
  failed. Photoshop unavailable, or a missing per-case Photoshop render, makes
  a P-case NOT RUN. ANY NOT RUN P-case ⇒ the drive prints `INCOMPLETE`, exits
  2 and never prints `ALL GATES PASS`; the run is then recorded as: P7-37's
  byte half **verified against the oracle only**, the Photoshop criterion left
  OPEN naming each NOT RUN case and its reason.
  **The GATE is at FILE level — the owner's decision, made in the fix pass
  after the first pass measured it.** D12 first gated each case on a READ-BACK
  of `knockout` / `blendInterior` through `getd`. **Photoshop 26.10.0 answers
  neither** — not in the bulk layer descriptor and not through a targeted
  property get, the same hole `blendRange` has — so that clause could never
  pass and it is REMOVED from the gate; the attempt is still made and its
  (empty) result is kept in the receipt as an OBSERVATION. What gates a case
  now is what Photoshop WROTE: for every layer the description SETS a key on,
  the record's `knko` body must equal the exact value for the requested mode
  (`00000000` none / `01000000` shallow / `02000000` deep) and its `infx` body
  the exact value for the requested interior flag (`00000000` off /
  `01000000` on). Evidence is keyed per RECORD INDEX with the name as a field
  (`blocks.json` schema 2), because three of the records are closing dividers
  all named `</Layer group>` and a name-keyed table collapses them; the ABSENCE
  of a block on a layer the description does not set is Photoshop's default
  and is not a failure. A silently ignored `setd` therefore still cannot
  become a MISS — the written body would not match — and the gate has no
  "differs from a baseline" clause of any kind. MATCH / MISS, T1 / T2 and the
  0 / 2 / 1 exit contract are unchanged.

- **D13 — D5's Background divergence, decided by 12E-3c as a DELIBERATE
  PRODUCT DECISION.** `D5's Background divergence is a DELIBERATE PRODUCT
  DECISION, not a measurement result: Rasterize keeps the structural
  Background rule and has no Background layer kind, so a Deep knockout (or a
  root Shallow one) over an ordinary opaque canvas-sized bottom layer shows
  that layer in Rasterize and transparency in Photoshop. It is disclosed on
  PSD import and export, measured as the DIVERGENCE case
  `deep_over_ordinary_bottom`, and owned as carryover P7-38 (10C-3).` The
  evidence it was decided on is the committed `knockout/source.psd`, record 0:
  a true Photoshop Background is the LOWEST record with NO channel id `-1`
  (ids 0, 1, 2 only), flags `0x09`, `lnsr` `bgnd`, `lspf` `0000000d`, while
  every ordinary bottom layer in the other four families has ids −1, 0, 1, 2
  and flags `0x08`. Rasterize never writes a true Background record. The
  disclosure fires on ONE predicate, the same on both sides but for the
  export's written clause: the
  file/document has no true Background record, its bottom entry satisfies D5,
  and a REACHING knockout exists — a visible entry whose style is Deep, or
  Shallow with no parent group. On the EXPORT side that entry's style must
  also be WRITTEN rather than rasterized: a BAKED style writes no `knko`, so
  the file it produces punches nothing in Photoshop and the sentence would be
  false beside the bake's own per-layer loss. Import evaluates it on the
  FRAMED records
  (`psd_group_records::structural_background_knockout`, no pixel decode);
  export on the document
  (`style_knockout::document_reaches_background` /
  `precision_style_knockout::document_reaches_background16` beside
  `background_entry` / `background_record`). Two known inexactnesses, recorded
  rather than hidden: at 16 bits a record-0 style the typed model refuses
  still counts as "styled" (one under-report), and a Deep inside an isolated
  group still counts as reaching (an over-report — which is why both sentences
  say "can").


## What 12E-3 verified

- `core/tests/knockout_composite.rs` (NEW, 20 tests) — tolerance stated in the
  file header BEFORE anything was measured: **T1 = 1 code of 255 per channel,
  alpha included**, against an oracle written in that file from D3–D6 in f64
  (`over` is the W3C source-over equation transcribed from the specification,
  `mix` D3's premultiplied lerp, and the separable blend functions come from
  the independent `tests/common/native_blend_oracle`). Claims: a root
  Shallow/Deep over a qualifying Background at fill 0, 0.5 Multiply and 1;
  each of the eight D5 disqualifiers (hidden, opacity < 1, a blend mode, an
  offset, a style, an enabled mask, a clipped entry 1, smaller than the
  canvas) leaving the backdrop transparent; a pass-through group giving
  Shallow the state its children began with and Deep the Background; the
  gated pass-through borrowing the untouched accumulator and still carrying
  Deep; an isolated group as a HARD STOP for both modes; a pass-through group
  nested inside an isolated one keeping both boundaries; a clipped member
  punching to its base alone over an UNSTYLED base and over a STYLED
  (drop-shadow) base at opacity 1 AND 0.5 (the direct and package-strip
  paths); a knockout base with a knockout member; a knockout GROUP; a
  partial-alpha shape getting no fringe at `q = 1` (asserted BOTH as equality
  with the plain render and against the oracle); Blend If as opacity and
  never shape, with Underlying read from the accumulated composite; Below
  effects and the opacity package untouched; `blend_interior_as_group` off vs
  on at fill 1 and 0.5 with an inner shadow still drawn outside the unit; the
  clip-base narrowing; D7 for all five merge-family ops; the `linear_light`
  refusal; the no-key byte-identity claim on a styled + grouped + clipped
  scene; and `projection_scratch_estimate_bytes` growing by D8's snapshots.
- `core/tests/knockout_format.rs` (NEW, 10 tests) — conditional emission
  (D9), the strict/lenient parse rules, D2 both ways, `.rz` word 49 inside 45
  with NO tail of its own, clearing the last key restoring the prior writer's
  EXACT bytes, reopen → resave byte-identical, both of D10's hand-patched
  refusals plus a bare 49 refused by name, the typed and float refusals by
  sentence, and the PSD export's per-layer loss with the written bytes equal
  to the same document's without the keys.
- `scripts/verify_phase12e3_knockout.py` (NEW, ONE owned instance) — **G1–G8
  all PASS**: the byte compositor against the drive's own f64 oracle within
  T1 (including the styled-base clip scene at both opacities), one
  `undo`/`redo` per change with an `unchanged` echo, a recorded
  `set_layer_style` replayed through `run_action`, the `.rz` round trip with
  word 49 present only with a key, the 16-bit and float refusals, the
  `linear_light` refusal, the PSD export loss and the Photoshop `source.psd`
  import disclosure, and the D7 note on BOTH `merge_down` branches (absent
  without a knockout, and absent from `merge_visible` in both scenes, because
  Merge Visible bakes knockout exactly) with `merge_down`'s catalog sentence
  in `tools/list` and `merge_visible`'s entry stating the exact bake.
- **The ten Photoshop P-cases: ALL TEN MATCH**, against Photoshop 26.10.0's
  own 8-bit render of each scene, under **T2** (2 codes of 255; 4 for the
  Blend If case), both sides matted on white. The file-level gate passed on
  every described layer — `RootShallow` record 2 `knko` `01000000`,
  `FillHalfMultiply` 3 and `BlendIf` 4 `02000000`, `PassKnockA` 7 `01000000`,
  `PassKnockB` 11 and `IsoKnock` 15 `02000000`, `ClipKnock` 19 `01000000`,
  `InteriorOn` 21 `infx` `01000000` — and the read-back stayed `<absent>` on
  all eight, recorded as an observation. Measured: `fill_half_multiply`
  max 1.0 / mean 0.1132, and **max 0.0 / mean 0.0** on the other nine
  (`root_shallow`, `group_shallow`, `group_deep`, `group_deep_hidden_bg`,
  `isolated_group_deep`, `clip_shallow`, `interior_off`, `interior_on`,
  `blend_if_knockout`). The comparison is not vacuous: at the probe pixel
  `root_shallow` shows the Background's own gradient (129, 164, 141) where a
  dropped knockout would show Mid, `clip_shallow` shows the clip base
  (204, 204, 34) where it would show the lower member, and `interior_on`
  (0, 164, 0) differs from `interior_off` (0, 255, 0). The drive prints
  `ALL GATES PASS` and exits 0.
- `cargo fmt --check`, `make lint`, both new test binaries, `make test`
  (whole suite) TWICE, `make typecheck` (0 errors), `make app`, the seven
  prior drives re-run green (12E-1, 12E-1b, 12E-1c, 12E-2, 12E-2b, 12E-2c,
  12D-1), and the public MCP reference regenerated from an owned instance and
  `--check`-clean. The five committed PSD fixture families are byte-identical
  (`git diff --quiet`). Receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12e-3/`.

**Recorded as misses or deviations rather than smoothed over.**

1. **Photoshop 26.10.0 reads back NEITHER key, so the gate reads the written
   RECORD instead — and with that gate every P-case RAN.** Neither the bulk
   layer descriptor nor a targeted property `getd` carries `knockout` or
   `blendInterior` (the same hole `blendRange` has, Run 3's finding); the
   attempt is still made and its empty result is in the receipt as an
   observation. The SET demonstrably takes, and that is what the file-level
   gate checks: `knko` = `00000000` / `01000000` / `02000000` for none /
   shallow / deep and `infx` = `00000000` / `01000000`, per RECORD INDEX, in
   the committed `core/tests/fixtures/psd_photoshop/knockout/blocks.json` —
   which is also D11's data requirement for 12E-3b. Nothing was widened; the
   tolerances are 12E-3's own, fixed before measuring.
   **The first pass of this run ALSO hard-coded every P-case to NOT RUN with
   the comparison body unwritten, and disclosed only the read-back hole as
   the reason.** That is recorded here because it is exactly the shape of
   omission this section exists for: the drive printed the right status for
   the wrong reason, and a reader would have concluded the comparison existed
   and was blocked. The comparison body — open `source.psd`, read each
   described layer's imported style, merge the two keys, send the whole style
   back, assert the imported `fill_opacity` / effects / `blend_if` survive,
   render, compare against Photoshop's own per-case render — is implemented
   now, and the per-case renders are committed with the family.
2. **D7's first Merge Visible clause was WRONG for this codebase, and D7 was
   corrected rather than the code.** `doc_structure::merge_visible` bakes
   `self.flattened()` — the document's OWN projection, the one accumulator
   that carries the Background snapshot — not a private transparent one like
   Merge Down and Merge Layers. So at 8 bits Merge Visible bakes knockout
   EXACTLY, as Flatten and Stamp Visible do, and
   `knockout_composite.rs` pins that rather than the hole D7 first predicted.
   The first pass nevertheless shipped the reply `note` and the catalog
   sentence on `merge_visible` and called them "conservative": they were not
   conservative, they were FALSE — they told a caller the picture could change
   when it cannot. Both are REMOVED. `merge_visible`'s reply carries no note,
   its catalog entry says it bakes knockout exactly, the header, the module
   doc, the README and D7 itself say the same, and the drive's G8 asserts the
   ABSENCE of a note there. Only Merge Down / Merge Layers are narrowed, and
   12E-3b's contract (g) makes those two exact.
3. **A drive-side trap worth recording:** the MCP `clear_selection` tool
   ERASES the selected pixels (it is Edit ▸ Clear, not Deselect). An early
   draft of the drive called it to tidy up between fills and silently
   emptied the layer it had just painted, which read as a compositor defect
   for several rounds. The drive now leaves its full-canvas selection active
   and VERIFIES every fixture layer as it is painted, so a construction
   failure can never again be reported as a finding.
4. `scripts/verify_phase8f.py` is **NOT RUN** again: its corpus is absent
   from this machine. **Owner 13D**, criterion unchanged.

## 12E-3b scope

12E-3b is the cut of 12E-3's remaining contract that does NOT need Photoshop:
(a) the TYPED twin of knockout / "Blend Interior Effects as Group", (b) the
typed `.rz` word **50**, (e) the Blending Options UI with the UI-probe float
opener, (g) EXACT merges (Merge Down / Merge Layers against the real backdrop
at both depths, Merge Visible kept exact), the typed half of (f) and the three
carried items. **12E-3c** keeps (c) PSD `knko`/`infx` at both depths, (d)
`cmls`, the 16-bit Photoshop re-measurement and P7-37's closure; its criteria
are recorded under "Open, with owners" below. **12E-3b drives Photoshop for
NOTHING**: the 8-bit P-cases re-run from the committed fixture family, so no
gate here is NOT RUN for Photoshop's sake. P7-37 stays OPEN after this run and
plan 12E is NOT marked "Shipped" until 12E-3c.

### The eleven decisions, recorded BEFORE the code

**(i) D5's Background divergence is decided by 12E-3c, not here.** Photoshop
tells a true Background layer from an ordinary opaque bottom layer and
Rasterize cannot, because a true Background is recognizable only in a PSD
record — so the decision belongs with PSD carriage. D5 is UNCHANGED in this
run, and the TYPED twin is D5 verbatim plus one extra clause, "no Blend If":
the byte rule's "no style" clause already covers Blend If (the byte model
carries it inside the style), while at 16 bits Blend If is a LAYER field and
must be named separately.

**(ii) D6's clip-base narrowing is KEPT, and CLOSED by decision, at both
depths.** Lifting it needs two things Adobe does not document: the order of
grouped interiors against clipped members, and whether fill dims the members.
Rather than guess, the narrowing stands and is DISCLOSED AT THE POINT OF
ACTION — `LayerStyleKnockoutRows.clipBaseNote` is shown under the checkbox
whenever the selected layer is a clipping base with visible clipped layers.
One OBSERVATION P-case (`interior_on_clip_base`) is measured for it — 12E-3c
did NOT run it and it is re-owned to **12E-3d** with the Photoshop block; that
case is an observation, never a gate.

**(iii) The `merge_visible` depth floor gets its own sentence.** When
`doc.isHighDepth && contributingLeaves().count >= 2 &&
contributingRasterLeaves().count < 2`, the MCP `merge_visible` no-op `why` is:
"fewer than two RASTER layers contribute to the picture — in a 16-bit or
32-bit float document Merge Visible needs two contributing raster layers, and
an adjustment layer contributes to the picture without counting toward that
floor (an 8-bit document counts it)." That closes the 12E-2c carry-over "The
MCP `merge_visible` no-op wording at depth".

**(iv) The typed STORAGE rule for a style carrying the two keys.** A typed
style is stored iff it is non-identity WITH BLEND IF REMOVED — `fill < 1 ||
some enabled effect || knockout != none` — and it then carries BOTH keys
verbatim. Consequence, stated rather than smoothed: a style whose only
non-default content is `blend_interior_as_group` beside a Blend If is DROPPED
at 16 bits, because it renders nothing at either depth (the flag needs a
grouped interior to move).

**(v) Group members of a merge.** BYTE: `doc_group::materialized` →
`rendered_group` keeps D4's hard stop, so a merged group — isolated or Pass
Through — is rasterized with `KnockoutBackdrop::NONE`. That is EXACT for an
isolated group, whose hole already shows the merged lower entry. 16 BITS:
`members_document` clones the subtree verbatim and a Pass Through member STAYS
Pass Through at the derived root, so its children read the derived document's
own entry state (Shallow) and the `Handed` deep plane (Deep, absent unless a
DIRECT member wanted the capture) — and NEVER D5, because every members
document renders `Handed`. So the typed merge does NOT isolate a pass-through
member; the earlier draft of this decision claimed it did and was wrong. At
BOTH depths a knockout nested inside a merged pass-through group can change
the picture, and decision (xi)'s R2 discloses it.

**(vi) The stated LIMIT of (g).** A merged raster cannot un-draw what lies
between, so exactness holds wherever `B0` is PRESENT and OPAQUE under the
shape. Where `B0` is ABSENT BY STRUCTURE the hole bakes as transparency:
inside an ISOLATED group `B0` is always transparent (D4's hard stop), so a
knockout merge there with a visible sibling beneath the merged set is NEVER
exact — before the merge the hole shows what lies beneath the GROUP, after it
shows that sibling. The same holds at the root of a document with no
qualifying Background. The module doc, this section and ONE byte test (G8's
isolated-group scene) pin it.

**(vii) Contract correction.** Merge Layers' private accumulator is
`doc_structure.rs::merged_layer`, not `doc_group_ops.rs`; `doc_group_ops` is
untouched by this run.

**(viii) Float.** A float document refuses styles by the existing
`"a layer style or Blend If"` / 32-bit float sentences, so the five merge
scenes are 8- and 16-bit only; at float the gate is the REFUSAL plus unchanged
merges.

**(ix) D6's "visible members" is a STRUCTURAL question and reads the
DOCUMENT's `visible` flags at BOTH depths.** `Cut`'s own contract
(`style_composite.rs`: "every structural decision is the one the screen made")
and the same input `is_isolated` takes. The SHIPPED byte predicate read the
cut-RESOLVED members, so a retouch plate whose cut excluded every clipped
member evaluated the flag ON and redrew the BASE — pixels at or below the
target — differently from the screen. That is CORRECTED here:
`doc_group::composite_unit_into` hands its existing `has_visible_clipped_members`
down as `CompositeEnv::clip_members_visible`, `doc::merging_down` sets it true
on its clipped arm, and the typed scene builder uses its existing `isolated`
local. `style_knockout.rs`'s member-loop test (`group.iter().any(|l|
l.visible)` inside the unit pass) stays on the RESOLVED members — that one IS
a contribution question, not a structural one. Declined alternative:
`!members.is_empty()`, which would copy the byte deviation to 16 bits and
still disagree with byte for a visible clipped group that renders nothing.
**Known residue, recorded here only:** Merge Layers materializes a visible
EMPTY clipped group as an INVISIBLE placeholder, so D6 can differ there.

**(x) Budget accounting is D8-style: a charge lives exactly as long as its
buffer**, with ONE named peak check — inside `KnockoutCapture::offer`, at the
one moment the nest, its snapshots and the copies coexist. After it the
projection only unwinds (the early returns allocate nothing), so that moment
is the maximum. The window charge travels INSIDE `MergeBackdrop` so it is held
across the merge's own composite.

**(xi) Residual disclosure replaces the wholesale removal of D7's note.**
`LayerStyle.knockoutMergeResidualNote` is asked BEFORE the edit by both
`merge_down` branches and is non-nil iff some VISIBLE entry `E` in the merge
set's subtrees has `knockout != .none` and one of:

- **R1** `E` is a ROOT of the set, a visible sibling lies below the set's
  lowest root in its level, and (`E` is Deep and ANY ancestor group of that
  level `isIsolatedGroup`, or `E` is Shallow and the level's PARENT group
  `isIsolatedGroup`);
- **R2** `E` is NOT a root and every group from its member root down to its
  parent is NOT `isIsolatedGroup` (a merged pass-through group — decision v);
- **R3** the lowest root is CLIPPED and an unclipped sibling lies below it in
  its level (its base is outside the set, so the merge runs with `NONE`).

`isIsolatedGroup(idx)` = group && (`blendMode != RZ_BLEND_PASS_THROUGH` ||
`layerHasStyle(idx)` || `layerClipped(idx)` ||
`isClipBaseWithVisibleMembers(idx)`) — a host MIRROR of `doc_group::is_isolated`,
named as such in its comment and deliberately conservative (it ignores
`baseless`). The predicate is CONSERVATIVE: it may fire on a merge that is in
fact exact (R2 at 16 bits with nothing between). It is SILENT on two stated
cases: the ROOT of a document with no qualifying Background (the host cannot
evaluate D5 — **owner 12E-3c**, which decides D5), and a PRESENT `B0` that is
not opaque under the shape (pixel-dependent; the catalog sentence says
"wherever that backdrop is opaque"). The three MENU items gain NO message
surface: the canvas shows the result and one Undo restores it. An MCP caller
has no canvas, which is why the REPLY carries the note.

### What the typed twin restates, and what it does not

`style_knockout.rs` owns every byte RULE; `precision_style_knockout.rs`
restates none of its own. It calls `is_grouped_interior`, `routes_unit`,
`grouped_interior`, `composite_contribution`, `clamp_alpha_to_shape`,
`composite_source_into`, `mix_premultiplied` and `source_weight_16`, and adds
NO pixel math. The one genuinely typed piece is the SEED: `KnockoutSeed` is
either `Projection { background }` (the document's own render, where D5 may
fire) or `Handed { shallow, deep }` (a derived members document, where D5
must NEVER fire — otherwise a nested knockout inside a Pass Through member
would take the lowest member for a Background).

### `.rz` word 50

`46 → [47] → [48] → [50] → 44 → …`, with NO table of its own: the two keys
ride inside the style JSON the `LSTY` record already carries. Both reader
sentences are evaluated ONCE and UNCONDITIONALLY, immediately after the
OPTIONAL `if has_style { … attach_style … }` block, so a file with NO `LSTY`
table is checked too: `has_50 != (some attached style carries a key)` refuses
with `"invalid native knockout word"` (50 with no keyed style) or
`"native knockout styles require RZDC 50"` (such a style with no 50), before
any PNG or f32 payload decodes. Clearing the last key restores the 12E-1c
writer's exact bytes.

### A recorded NON-defect: Pass Through on a fresh typed group

12E-3b's first pass recorded "a typed group cannot be set to Pass Through over
MCP" as a pre-existing host defect, blaming
`NativeLayerBlending.allows`'s `layerIsGroup`. **That diagnosis was wrong and
the review pass MEASURED the real answer.** `layerIsGroup` DOES recognize a
typed group: `RasterDocument+PrecisionMetadata.swift` maps every kind ending
in `group` — `pass_through_group` included — to `RZ_LAYER_GROUP`, so `allows`
admits Pass Through. What answers the call is the CORE'S PURITY RULE: a fresh
typed group is already Pass Through, `with_blend_mode` returns `None` for a
value the layer already carries, and `nativeLayerProperties` reports
`changed: false` with "Nothing changed: the layer already carries every
requested property". Measured on an owned instance at both depths: the fresh
group answers that no-op, and setting Normal and then Pass Through BACK
succeeds at 16 bits — so the operation works and there is nothing to fix or
own. The drive's `group()` helper says exactly this, and asks for a blend mode
only when the group has one to change.

## 12E-3c scope

PSD `knko` / `infx` in BOTH directions and at BOTH depths, D5's Background
divergence decided as a disclosed divergence (D13 above), `cmls` re-owned, and
the Photoshop re-measurement. **The ten decisions were recorded here BEFORE any
Photoshop work started** (the run's CHECKPOINT), and this run drove Photoshop
for NOTHING: the Photoshop block is re-owned to **12E-3d**, below.

### The ten decisions, recorded BEFORE the code

- **E1 — the codec.** `knko` = `[k,0,0,0]`, k 0/1/2 = none/shallow/deep;
  `infx` = `[0|1,0,0,0]`; any other framing is NOT carried and keeps today's
  marker sentence. `clbl` stays observation-only (D11). Both keys ride the
  record's ONE `LayerStyle`, on rasters and OPENING folder records (`lsct`
  1/2) at both depths; a CLOSING divider (`lsct` 3) never carries either key —
  its decoded style is attached to nothing, so its marker loss stays. An
  identity result (a lone `infx`, D2) stays `Absent`. Written only when
  non-default, after `iOpa`, order `infx` then `knko`, before `lclr`.
- **E2 — the bake.** A BAKED style (`ExportStyle.bake.is_some()`) writes
  neither block and reports a narrowed per-layer loss; 12E-3's "neither is
  written to a PSD yet" sentence is DELETED at both depths.
- **E3 / D13 — D5 KEPT STRUCTURAL: no Background flag, no `.rz` change, no
  FFI. OWNER DECISION, BINDING.** D13 above carries the evidence, the
  predicate and the two known inexactnesses.
- **E4 — the ROOT case closes with E3:** the host gains a named MIRROR of D5
  (`RasterDocument.hasQualifyingBackground`) and the merge residual note gains
  **R4**; the note constant and the `merge_down` catalog sentence are extended
  to name it.
- **E5 — `deep_over_ordinary_bottom` is a DIVERGENCE case (PD), not a
  MATCH/MISS gate — OWNER DECISION, BINDING.** Measured at 8 and 16 bits under
  a FOURTH status **DIVERGENCE (disclosed)**; the drive exits 1 unless all
  four conditions hold at that depth, an unpredicted result (including a
  whole-frame match WITHOUT the control) is a FAILURE, and Photoshop
  unavailable is NOT RUN. **NOT MEASURED by this run — re-owned to 12E-3d.**
- **E6 — export verification = Photoshop RE-SAVE + RENDER CONTROL, per keyed
  record (PX).** **NOT MEASURED by this run — re-owned to 12E-3d.**
- **E7 — `cmls` is NOT imported this run; the loss is re-recorded, owner
  10C-3.** Evidence: 137 PSDs scanned (repo + `~/devel/rasterize-fixtures`),
  none contains `cmls` or image resource 1065; the harness cannot author
  comps and `psd_records.py` exposes neither resource bodies nor `shmd` items;
  Adobe's document defines Layer Comps as resource 1065 (a version-16
  descriptor) and leaves the per-layer rows to the undocumented `shmd` list,
  so the contract's "comps naming layers by record index" premise — and the
  12D doc's "by a persisted id" — are both UNVERIFIED. The EVIDENCE family
  this decision asked for was the first drop candidate and **was not authored
  — owner 10C-3.**
- **E8 — the 16-bit control floor, fixed before measuring:**
  `CONTROL_MIN_CODES = 32` = `CONTROL_MIN_CODES_16 = 8224`; `interior_off`
  exempt as at 8 bits. T3 vs Photoshop: max RGB ≤ 2 8-bit-equivalent codes
  (the 16-bit difference ÷ 257), `blend_if_knockout` ≤ 4, both sides matted on
  white. The floor is written into `verify_phase12e3c_psd.py`; T3 belongs to
  the re-owned block.
- **E9 — the BLEND_OPTIONS observation cannot claim a carried `infx`.**
  `FeatureMarkers` tracks `clbl` and `infx` SEPARATELY now. Every record whose
  `infx` is NOT carried reports the pinned sentence byte-for-byte as before;
  where `infx` IS carried, an `infx`-only record reports no BLEND_OPTIONS
  sentence and a record that also carries `clbl` reports ONE new sentence
  instead (`… has additional clipping blending metadata (clbl); its payload is
  not interpreted or retained, and its presence alone does not establish an
  active effect.`). The carried key is named only by `report_style`. A carried
  `knko` clears KNOCKOUT from `losses` AND `observations`.
- **E10 — the ledger.** P7-37 → verified iff P16, every PX record verdict,
  PD, the C-gates and both contract observations land. **They did not**, so
  P7-37 stays OPEN naming P16, PD, PX and the two observations, ALL FOUR owned
  by **12E-3d** (the observations were re-owned there with the Photoshop
  block: both need Photoshop and `interior_on_clip_base` needs the
  `knockout_bottom` family only 12E-3d authors). `cmls` and P7-38 are
  **10C-3's** and do NOT hold P7-37 open. P7-38 is added as a new open row;
  the counts read THEN **twenty-seven verified, 11 open** (12E-3d closes P7-37
  and adds P7-39, so they are twenty-eight / 11 now); and plan 12E reads
  "12E-3c shipped; 12E open for 12E-3d/10C-3".

### What shipped

- The ONE codec (`psd_layer_effects.rs`) gained `decode_knockout` /
  `decode_interior`, the two `RecordStyle` / `Carried` fields,
  `knockout_blocks`, `knockout_clause` / `knockout_keys` and the module-doc
  section that IS the mapping table.
- `psd_group_records.rs` frames the first `knko` / `infx` body per record and
  passes them ONLY for dividers 0/1/2, calls `FeatureMarkers::carry_knockout`,
  and owns `BottomRecord` + `structural_background_knockout`.
  `psd_raster_masks.rs` gained `MaskRecord::enabled`.
- Both export walks write the blocks after `iOpa` and before `lclr`, on
  rasters and on OPENING folder records, only when the style is WRITTEN and
  the key is not at its default; `style_block_bytes` charges 16 bytes per
  block; the preflight carries the written-style observation, the baked loss
  and D13's per-file loss, and the two general limitations are APPENDED to.
- Swift: `hasQualifyingBackground` (the named D5 mirror), R4 in
  `knockoutMergeResidualNote`, the extended note constant, the `merge_down`
  catalog sentence and the `export_psd` catalog text.
- `core/tests/psd_knockout.rs` (NEW, 7 tests) checks the committed
  `knockout/source.psd` against its own `blocks.json`, the 8- and 16-bit round
  trips against this file's OWN `8BIM<key>` byte scan, the set-then-clear byte
  identity, the bake, D13's two sentences (present / five ABSENT cases,
  the last of them a Deep knockout whose style is BAKED — no `knko` is
  written, so that file punches nothing in Photoshop and the disclosure must
  stay silent beside the per-layer "it is not written" loss) and the framings
  that are not carried — including the closing divider whose marker loss
  stays.
- `scripts/verify_phase12e3c_psd.py` (NEW) drives the C-document at 8 and 16
  bits (C1/C2) and R4 (C3), each gate in its own `try` so one failure still
  leaves the others measured. Its Photoshop half is NOT IMPLEMENTED and is
  reported as such.
- `scripts/photoshop-probe/psd_records.py`'s record-block walk now picks the
  pad by inspecting the bytes: **Rasterize's EVEN pad is exactly what Adobe's
  table requires, while Photoshop pads its additional layer information to a
  MULTIPLE OF 4, and the reader reads BOTH** — the 4-pad skip stands unless it
  fails to land on a signature where the 2-pad does, which a 4-padded writer's
  zero pad bytes cannot produce. (The `resource_bodies` / `shmd` change the
  spec named belongs to `cmls`, owner 10C-3, and was not made.)

## 12E-3d scope

The 12E-3c Photoshop block, measured: **P16** (the ten P-cases re-measured at
16 bits), **PD** (`deep_over_ordinary_bottom`, D13's disclosed divergence),
**PX** (Photoshop re-save + render control per keyed record, with its padding
check) and the **two contract observations** (`interior_on_clip_base`,
Photoshop's own Merge Down). It extends `scripts/photoshop-probe/` with the
`resave` and `merge_down` jobs, derives the 16-bit `knockout` artefacts from
the committed `source.psd` and authors the `knockout_bottom` family.

This is a MEASUREMENT run. A Photoshop disagreement that is not a bug against
the recorded design (D1–D13, E1–E10) is recorded as a **MISS with its
numbers** and keeps P7-37 open naming it; it is never a reason to widen a
tolerance, lower a floor, re-author a scene or tune the compositor.

### The eleven decisions, recorded BEFORE the first Photoshop call

- **F1 — tolerances and floors.** T2 = 2 codes of 255 (`blend_if_knockout`
  4), imported from the 12E-3 drive. **T3 = 2 8-bit-equivalent codes
  (`blend_if_knockout` 4)**, 8-bit-equivalent = the 16-bit difference ÷ 257,
  both sides matted on white. `CONTROL_MIN_CODES = 32`;
  `CONTROL_MIN_CODES_16 = 8224` (= 32 × 257, so "max ≥ 32 8-bit-equivalent"
  and "max ≥ 8224 codes of 65535" are the SAME test). **Every comparison
  whose two PNGs are both 16-bit records BOTH numbers**: `max_rgb` /
  `mean_rgb` (8-bit-equivalent, the number the verdict is taken on) and
  `max_codes_16` / `mean_codes_16` (the same matted difference in codes of
  65535, from the UNROUNDED value), so a MISS is named at the depth it was
  measured and a sub-8-bit disagreement is visible in the receipt.
  `interior_off` is control-exempt. **PD:** Photoshop-white, outside
  agreement and the control under T2 (8) / T3 (16); `PD_BASE_CODES = 1`
  8-bit-equivalent (Rasterize vs `base.png` in the hole); **the PD hole floor
  is `CONTROL_MIN_CODES` / `CONTROL_MIN_CODES_16`, as the MAX RGB difference
  over hole pixels**; `PD_UNDO_CODES = 0` (all four channels, in the depth's
  own codes). **The PX render floor** is the same two floors. Nothing is
  widened after a measurement.
- **F2 — the 16-bit comparison tool** is `scripts/photoshop-probe/pixels.py`'s
  `png_rgba` (pure `zlib`; colour types 2 and 6 at 16 bits) plus the 12E-3
  drive's `compare_on_white`. `magick` produces no number in any receipt.
  Both PNGs of a 16-bit comparison are ASSERTED `depth == 16`.
- **F3 — the 16-bit artefacts are DERIVED, never re-authored.** The drive
  copies the committed `knockout/source.psd` to `<work>/photoshop/p16_input/`
  and asserts that copy's sha256 equals the committed `knockout/photoshop.json`'s
  `files['source.psd']` (`e54642355055…b62e1` — the hash Photoshop itself
  recorded when it wrote the file). ONE `resave` job (depth 16, name
  `source16.psd`, the ten `CASE_VISIBLE` renders) writes `source16.psd`, the
  ten `<case>_16.png` and its report; the drive copies that report to
  `photoshop16.json`, copies the family's `description.json` beside them and
  writes `blocks16.json` from `psd_records.read(source16.psd)`.
  `photoshop16.json` IS committed: it carries `application.version`, the
  `files` sha256 table of all eleven outputs and `resave.input_sha256`. The
  committed `source.psd` is only ever opened as that copy.
- **F4 — the `knockout_bottom` family** (one `author` job, depths [8, 16]).
  PNGs written by the drive: `base.png` = `pixels.base_picture(64,64)`;
  `mid.png` / `extra.png` = 64×64 solid MID / EXTRA; **`high.png` = 32×32
  solid HIGH, alpha 255** (the `masks` / `depth16` precedent: a smaller opaque
  PNG at an `offset`; a canvas-sized solid would make the OUTSIDE region empty
  and PD's condition 3 vacuous). Description (schema 1, canvas [64,64], sRGB):
  `base: {"name":"Bottom","png":"base.png"}` — **NO `background` key**; layers
  bottom-first, all opacity 100, visible: `Mid` (`mid.png`, [0,0], normal) ·
  `DeepKnock` (`high.png`, offset **[16,16]**, normal, `knockout:"deep"`,
  `fill_opacity:0`) · `ClipBase` (`extra.png`, [0,0], multiply,
  `blend_interior:true`, `color_overlay {[0,255,0], 100, normal}`) ·
  `ClipMember` (`high.png`, [16,16], normal, `clipped:true`). Renders:
  `deep_over_ordinary_bottom` = `[Bottom, Mid, DeepKnock]`;
  `interior_on_clip_base` = `[Bottom, ClipBase, ClipMember]`. **`Bottom` is
  file record 0**: the opened RGBA base PNG is an ORDINARY layer, so its
  record carries channel ids −1,0,1,2 and flags `0x08` where a true Background
  has ids 0,1,2 only, flags `0x09` and `lnsr bgnd` (D13's evidence); its
  rectangle is `[0,0,64,64]` because `base.png` is opaque and canvas-sized —
  exactly D5's "size == canvas, offset (0,0)". The file therefore proves both
  halves: Photoshop has NO Background to punch to, and Rasterize's D5
  qualifies `Bottom`. **HOLE** = the canvas pixels covered by `high.png` at
  `DeepKnock.offset` where its alpha is 255 (asserted: every alpha ∈ {0,255});
  **OUTSIDE** = every other pixel; asserted `0 < |HOLE| < 4096`. Computed from
  the description, never from a render.
- **F5 — `--skip-photoshop` means Photoshop is never launched.** P16, PD and
  observation (a) are file-vs-Rasterize comparisons and RUN from the committed
  families; PX, its padding check and observation (b) need live Photoshop and
  are `NOT RUN: skipped by flag`. `--author` with `--skip-photoshop`, and
  `--install-fixture` without `--author`, are argparse errors.
- **F6 — `cmls_evidence_family` leaves the exit accounting.** It is "never a
  gate", owner 10C-3, and would make exit 0 unreachable. It moves from
  `NOT_IMPLEMENTED` to a receipt key `not_in_this_drive` and one printed
  `NOTE` line.
- **F7 — the final line.** With no failure and no NOT RUN: `ALL GATES PASS`
  iff zero MISS, else one `MISS …` line each (at depth 16 with both numbers:
  `max=<max_rgb> (<max_codes_16> of 65535)`), `MISSES: n` and
  `GATES PASS WITH MISSES`; exit 0 either way (D12: a MISS is a passed gate
  over tolerance).
- **F8 — the residual owner.** Any MISS / FAILURE that survives a check of the
  drive, any NOT RUN and any dropped item keeps P7-37 OPEN naming it with its
  numbers, owner **12E-3e**, criterion verbatim; plan 12E then reads "12E-3d
  shipped; 12E open for 12E-3e: <items>" and the counts stay 27 / 11.
- **F9 — Rust / Swift: NOTHING changes.** Only if a measurement exposes an
  outright bug against D1–D13 / E1–E10: fix it in the owning module, add an
  oracle test that FAILS before the fix (`core/tests/psd_knockout.rs` for the
  codec, `knockout_composite.rs` / `precision_knockout.rs` for the
  compositor), re-run the measurement. Never tuned toward Photoshop, never a
  widened tolerance. A Photoshop disagreement that is not a bug against the
  recorded design is a MISS.
- **F10 — the drop order if budget runs short, from the END:** observation (b)
  (and the `merge_down` job) → observation (a) → PX (with its padding check) →
  PD. **P16, `resave` and both authoring jobs are never dropped.** A dropped
  item stays a `NOT_IMPLEMENTED` entry (exit 2) and F8 applies.
- **F11 — the tight PX case, PREDICTED before measurement: `InteriorOn` ≈ 51
  codes (13107 of 65535) against the 32 / 8224 floor.** Over `Mid`
  (34,204,34) with `[Bottom, Mid, InteriorOn]` visible: keyed (`infx` on) =
  overlay then Multiply = (0,204,0); cleared = Multiply then overlay =
  (0,255,0) — the cleared export keeps the overlay and drops only the key. G
  differs by 51, only 19 codes above the floor. **A result BELOW the floor is
  a `MISS` for the `InteriorOn` record with its number** — Photoshop did not
  visibly honour the key in that scene — and F8 applies. It is NEVER a reason
  to lower the floor, and NEVER a reason to re-author or re-colour the scene
  until it clears. The prediction is recorded in the receipt beside the
  measurement.

### What ran, and what it measured

**Photoshop 26.10.0**, pinned. Session facts, identical on both
Photoshop-driving runs: `was_running false`, `launched true`, `quit true`,
`still_running false` — Photoshop was NOT running before the first harness
call and is NOT running after the last, so it was left exactly as it was
found. Receipts:
`~/devel/rasterize-fixtures/phase12-receipts/12e-3d/` (`smoke/`,
`drive-author/`, `drive/`, `drive-skip/`, `12e3_knockout/`,
`12e3b_typed_knockout/`, `9h.json`). Every number below is from
`drive/receipt.json`; `drive-author/receipt.json` carries the SAME P16 and PD
statuses, which is the point of running both.

**F9's statement: the MEASUREMENT changed no Rust and no Swift.** No measurement
exposed a bug against D1–D13 or E1–E10, so the conditional branch F9 allows
was never taken. Nothing was tuned toward Photoshop, no tolerance was widened,
no floor was lowered and no scene was re-authored. The review pass that
followed added ONE thing to the core: the exchange-side DISCLOSURE of
observation (a) on PSD import and export (P7-39), which changes no pixel, no
written byte and no evaluation.

**Known inexactness of that disclosure — CLOSED by
[12F-1](phase-12f-geometry-status.md)'s carried items (C1 and C2), whose four
`psd_knockout.rs` cases (h1)-(h4) pin the clipped-folder false positive, the
clipped-group miss, the "renders" question and the baked-style clause at 8 AND
16 bits. P7-39's own evaluation stays open (10C-3). As it stood:** the
IMPORT predicate `psd_group_records::clip_base_blend_interior` is not the same
condition as the export's. It lacks the sibling-level guard
`psd_export::clip_base` has, so (i) a clipped FOLDER record, which sits
directly above its own topmost child in file order, makes that child falsely
count a clipped member, and (ii) a clipped GROUP genuinely clipped to the base
is MISSED, because its closing divider is written unclipped and ends the run —
a Rasterize-written file can carry the export sentence and not the import one.
Both predicates also omit D6's other half (one of Gradient Overlay, Color
Overlay, Satin or Inner Glow must render), so a style with the flag on and
only exterior effects discloses a divergence that cannot exist. These affect a
REPORT SENTENCE in edge cases only: no pixel, no written byte. Accepted when
the import predicate reads clipped runs at the base's own level (a clipped
group through its folder record), both predicates require a rendering grouped
interior effect, the comment above the import predicate is true, and
`core/tests/psd_knockout.rs` pins the clipped-folder and clipped-group cases
on both sides.

**The harness.** `photoshop.jsx` gained `resave` (open, convert iff the depth
differs, `saveAs` AS A COPY under a new name, render at that depth;
`report.resave = {name, depth, converted, input_sha256}`, the input hashed
BEFORE the document is opened) and `merge_down` (render, Merge Down one named
top-level ArtLayer, render again, never save). `psd_records.py`'s records
gained `trailing` — the bytes left in the record's extra data after the block
walk; **0 on every committed Photoshop record and on every Rasterize record
measured here** (8 committed PSDs, 79 records).

#### P16 — the ten P-cases at 16 bits (T3 = 2 / 4 8-bit-equivalent codes)

`source16.psd` is DERIVED: one `resave` of a COPY of the committed
`knockout/source.psd`, whose sha256 `e54642355055…b62e1` is the hash the
committed `photoshop.json` recorded, carried forward as
`photoshop16.json`'s `resave.input_sha256` (`converted: true`). The document
opens `rgba16`; both sides of every comparison are 16-bit PNGs; the FILE gate
on `blocks16.json` passed.

| case | max (8-bit-equiv) | max (of 65535) | mean | control max | gated |
| --- | --- | --- | --- | --- | --- |
| `root_shallow` | 0.0 | 0.0 | 0.0 | 221.0 (56797) | yes |
| `group_shallow` | 0.0 | 0.0 | 0.0 | 169.9961 (43689) | yes |
| `group_deep` | 0.0 | 0.0 | 0.0 | 221.0 (56797) | yes |
| `group_deep_hidden_bg` | 0.0 | 0.0 | 0.0 | 221.0 (56797) | yes |
| `isolated_group_deep` | 0.0 | 0.0 | 0.0 | 169.9961 (43689) | yes |
| `clip_shallow` | 0.0 | 0.0 | 0.0 | 169.9961 (43689) | yes |
| `fill_half_multiply` | 0.0156 | 4.0 | 0.005 (1.27) | 198.8093 (51094) | yes |
| `interior_off` | 0.0 | 0.0 | 0.0 | 0.0 | no (exempt) |
| `interior_on` | 0.0 | 0.0 | 0.0 | 255.0 (65535) | yes |
| `blend_if_knockout` | 0.0 | 0.0 | 0.0 | 221.0 (56797) | yes |

**Ten MATCH**, nine with a non-vacuous control (every one far above the 32 /
8224 floor), `interior_off` exempt as at 8 bits. The ONE non-zero case,
`fill_half_multiply`, disagrees by **4 codes of 65535** — a sub-8-bit
rounding difference that the 8-bit run reports as 1 code and that is only
visible because every 16-bit comparison now records both units.

#### PD — `deep_over_ordinary_bottom`, D13's divergence (both depths)

The `knockout_bottom` file proves both halves of D13 at once. Record 0 is
`Bottom`, channel ids **−1, 0, 1, 2**, flags **0x08**, no `lnsr`, rectangle
`[0, 0, 64, 64]` — an ORDINARY layer, so Photoshop has no Background to punch
to; and it is canvas-sized at the origin, visible, Normal, opacity 1,
unmasked, unstyled with an unclipped entry 1, so Rasterize's D5 qualifies it.
`inspect_psd` carries D13's import loss verbatim. HOLE = 1024 pixels, OUTSIDE
= 3072, both derived from the description and `high.png`'s own alpha.

| | 8 bits | 16 bits |
| --- | --- | --- |
| hole: Photoshop vs white | max 0.0 | max 0.0 (0 of 65535) |
| hole: Rasterize vs `base.png` | max 0.0 | max 0.0039 |
| hole: Photoshop vs Rasterize | **max 255.0**, mean 103.6292 | **max 255.0 (65535 of 65535)**, mean 103.6293 (26632.74) |
| outside: Photoshop vs Rasterize | max 0.0 | max 0.0 (0 of 65535) |
| whole frame | max 255.0, mean 25.9073 | max 255.0, mean 25.9073 (6658.18) |
| control (mask on entry 0) vs Photoshop | max 0.0 | max 0.0 (0 of 65535) |
| `undo` | 0 codes | 0 codes |

**DIVERGENCE (disclosed) at both depths.** Photoshop punches the hole to
transparency (white in its PNG) EXACTLY; Rasterize shows `base.png` there
EXACTLY; outside the hole the two agree to the code; and disqualifying the
bottom layer with one reveal-all mask makes the WHOLE frame agree to the code
— which is the evidence that the divergence is D5 and nothing else. The
`undo` returns the render bit-for-bit at both depths.

#### PX — Photoshop's re-save and the render control, per keyed record

Photoshop re-saved the Rasterize-written `px_<d>_keyed.psd` (11 records;
`layer_info` at 8, an `Lr16` tail at 16) and rendered it, and rendered the
all-keys-cleared twin.

| record | written | re-saved | keyed vs cleared, 8 | keyed vs cleared, 16 |
| --- | --- | --- | --- | --- |
| `DeepKnock` | `knko 02000000` | `knko 02000000` (+ default `infx`) | 110.0 | 110.0661 (28287) |
| `PassKnock` | `knko 01000000` | `knko 01000000` (+ default `infx`) | 170.0 | 169.9961 (43689) |
| `GroupKnock` (FOLDER record) | `knko 01000000` | `knko 01000000` (+ default `infx`) | 221.0 | 221.0 (56797) |
| `InteriorOn` | `infx 01000000` | `infx 01000000` (+ default `knko`) | 51.0 | 51.0039 (13108) |

**Eight MATCH.** Every body Rasterize wrote survived Photoshop's re-save
byte-for-byte, the FOLDER record's `knko` included, and every keyed render
differs from its cleared twin by far more than the 32 / 8224 floor.
**F11's prediction was 51 codes (13107 of 65535) for `InteriorOn`; the
measurement is 51.0 at 8 bits and 51.0039 (13108 of 65535) at 16** — the
tight case cleared its floor by 19 codes exactly as derived, and the
prediction was written down before the first Photoshop call.
`getd` still answers neither key on 26.10.0 (`<absent>` for all four records,
both depths), which is why the gate reads the written record.

**PX-PAD: MATCH at both depths, two candidates each.** `DeepKnock` and
`PassKnock` both carry a 22-byte `luni` (≡ 2 mod 4). What **RASTERIZE WROTE**
after that odd-halfword name is `luni → iOpa → knko` — the drive's own
printed detail (`order luni->iOpa->knko`) and both `px_<d>_keyed.psd` files
say so — so `iOpa` and `knko` are the two blocks that sit after the name and
survive. Photoshop's **RE-SAVED** order is the longer
`luni → lyid → iOpa → clbl → infx → knko → lspf → lclr → shmd → fxrp`, and
the `infx 00000000` in it is a Photoshop DEFAULT that the gate deliberately
excludes from evidence: Rasterize never wrote an `infx` on either record, so
nothing about `infx` "survived" here. After
Photoshop's re-save each record's `knko` (`02000000` / `01000000`) and `iOpa`
(`80000000` / `00000000`) are the written bodies, the `luni` still decodes to
the record's own name, every key is four printable ASCII characters, no block
reads as an unknown signature, and `trailing` is 0. Photoshop reads
Rasterize's EVEN pad.

#### The two contract observations (never gates)

**(a) `interior_on_clip_base` — this is the measured COST of D6's clip-base
narrowing, and it is large.** At both depths the whole frame differs by **max
255, mean 34.078** (16-bit: mean 8758.1 of 65535); inside `ClipMember`'s
rectangle mean 21.243, outside it mean 38.357. Sampling explains it exactly:
over `base.png` Photoshop renders `Multiply(base, (0,255,0))` = `(0, g, 0)`
— the colour overlay blended INTO the layer and the layer then Multiplied
against the backdrop, i.e. "Blend Interior Effects as Group" honoured —
while Rasterize renders a flat `(0,255,0)`, the overlay applied AFTER the
blend. That is **exactly D6's recorded narrowing**: `style_knockout::
grouped_interior` returns false when the layer is a clip base with visible
members, so the flag is kept and evaluates OFF. That is the SHIPPED
behaviour and this run changes none of it — but 12E-3b's decision (ii)
CLOSED the narrowing "by decision" without this evidence in front of it, and
a full-scale measured disagreement with Photoshop is not something a decision
can close by itself. The decision is therefore SUPERSEDED as a decision by an
OPEN ROW — **P7-39**, owner 10C-3, in "Open, with owners" below, with a
measurable acceptance criterion — exactly as D5's divergence became P7-38.
P7-37 is NOT reopened: its verify column closes on both observations being
RECORDED, and converting an observation into a gate after the fact would be
dishonest in the other direction. Recorded and OWNED, not fixed — and
DISCLOSED on the exchange as of the review pass, once per file in each
direction, beside D13's two sentences and under the same framed-records-only
rule (no pixel is decoded): `psd_group_records::clip_base_blend_interior` on
the way in (a record whose carried style kept an `infx` that is ON, with a
VISIBLE record clipped to it) and the matching `layered_losses` clause on the
way out (a WRITTEN — never baked — `infx` on an entry whose
`ExportStyle::clip_base` is true). The key itself still crosses intact; only
its evaluation differs, and both sentences say so. Tested in
`core/tests/psd_knockout.rs` (g) for the present case and for all four
absent ones — no clipped member, the member hidden, the flag off, the style
baked — at both depths.

**(b) Photoshop's own Merge Down** of `[Background(true), MidA, MidB, Top deep
fill 0]`, at both depths, `Top` merged `into` `MidB`, 4 top-level layers
before and 3 after. Before the merge the two agree EXACTLY (before-vs-before
max 0.0 at both depths). **Photoshop's own merge moves its picture by max
221.0 (56797 of 65535), mean 106.76; Rasterize's `merge_down` of the same
imported file moves nothing at all (max 0.0) and carries no `note`** — the
document has a qualifying Background, so 12E-3b's contract (g) merges the
knockout against its REAL backdrop and R4 stays silent. After-vs-after is
therefore max 221.0. Photoshop bakes the hole of a merged Deep knockout as
transparency even where a Background exists; Rasterize keeps the picture.
Recorded, not fixed — it is 12E-3b's decided behaviour, and the reply is
exact rather than disclosed precisely because nothing was lost.

#### Coverage and exits

`drive-author --author --install-fixture` and `drive` both exit **0** with
`ALL GATES PASS`; `drive-skip --skip-photoshop` exits **2** with `INCOMPLETE`
for exactly the three live items (the eight PX record verdicts, the two
PX-PAD verdicts and observation (b)) — P16, PD and observation (a) RUN from
the committed families with the same numbers. `cmls_evidence_family` prints
one `NOTE` line and is owner 10C-3's, never a gate (F6). The 13 `knockout`
additions and the 14 `knockout_bottom` files are the only fixture changes;
every previously committed file under `psd_photoshop/` is byte-identical.

**Recorded, not fixed** (none is a bug against the recorded design, and each
now has an owner): observation (a)'s D6 narrowing cost above — which had NO
row, no owner and no criterion when this run first wrote this list, and is
owned from now on by the new **P7-39** (10C-3) below; observation
(b)'s Merge Down difference above; the stale module-doc sentences at
`core/src/precision_layer_blend.rs:9-11` and `core/src/style_knockout.rs:
117-118`, which a comment-only edit would force through F9's whole chain —
owner: the next run that edits either module.

## Open, with owners

- ~~**The 12E-2b drive's per-gate relaunch — owner 13A.**~~ **CLOSED by 13A-7 (2026-09-25):** the drive runs every gate in ONE owned instance (the per-gate relaunch removed) and its new P11 holds the count of threads parked in `-[NSAnimation _runBlocking]` equal to its starting value (0) after P10 and at every 20 of 200 more worker-backed edits, with no Dispatch Thread Soft Limit; ALL GATES PASS ([13A status](phase-13a-regional-status.md), 13A-7 verification record). The AppKit defect
  behind it (one threaded `NSAnimation` parked per `BackgroundWork` panel
  presentation) is FIXED and measured in the 12E-2b review pass (deviation 3
  above), so `scripts/verify_phase12e2b_placement.py` no longer needs a fresh
  instance per gate; the drive was simply not re-shaped in that pass. 13A
  already owns migrating the preparation stages this panel serves "to measured
  cancellation/progress", so it owns this too. **Accepted when:** in ONE owned
  instance, 200 consecutive worker-backed edits that each present the panel
  leave the count of threads parked in `-[NSAnimation _runBlocking]`
  (measured with `sample`) flat at its starting value — the measurement the
  fix already passes — AND `verify_phase12e2b_placement.py` passes run against
  a single instance (a `--single-instance` mode or equivalent), after which
  the per-gate relaunch is removed.
- **The Phase 8F fixture corpus — owner 13D.** `scripts/verify_phase8f.py`
  has been NOT RUN since its corpus went missing: it reads
  `/tmp/rz-phase8f-fixtures`, built by `scripts/host-source-probe/fixtures.py`
  from the preserved `/tmp/rz-phase8f-feasibility`, and both are absent (a
  `/tmp` corpus does not survive the machine). 13D owns scale/corpus
  qualification. **Accepted when:** the 8F fixture corpus is re-authored by a
  COMMITTED script into `~/devel/rasterize-fixtures/phase8f/` (never `/tmp`),
  `verify_phase8f.py` reads it from there, and the drive passes. Recorded in
  `designs/phase-8f-host-source-status.md` too.
- **12E-1c — CLOSED.** Guides and the ruler origin at depth shipped in this
  run (the scope section above); the acceptance criteria it was written
  against are each met there, with the two deliberate exclusions — typed
  Canvas Size / `padded_*` and canvas geometry at float — recorded rather than
  silently skipped. BOTH are closed by
  [12F-1](phase-12f-geometry-status.md): the typed `CanvasSize` op calls
  `padded_guides`/`padded_origin` at both depths, and `canvas_size` is
  admitted at float behind an explicit gate. P7-26 is closed with it.
- **The AppKit Shift-equivalence rule — CLOSED by measurement, not carried.**
  12E-1b measured it in both event shapes (M1–M7 above). On the shape the
  window server delivers, a key equivalent matches the chord exactly, so
  `ShortcutMenu.conflict`'s exact comparison is right, the shipped ⌘Z/⇧⌘Z,
  ⌘S/⇧⌘S and ⌘E/⇧⌘E pairs are not shadowed, and the "shipped defect" the
  first 12E-1b pass recorded did not exist. The one thing a later run may
  still want — measuring each shipped pair with REAL enablement in a key
  window rather than with a probe target — is a strengthening of the probe,
  not an open decision.
- **Typed locks on the retained-source and RAW edit families — CLOSED by
  12E-2b**, with the disclosure itself corrected: the BYTE
  `doc_retained_source.rs` had consulted `lock_block` since 8A and only
  lacked tests; the real gaps were the TYPED twin and the attachment writers
  at both depths. The table and the tests are in the 12E-2b sections above.
- **12E-2 — CLOSED.** Positional insertion at all three positions at 8, 16
  and 32-bit float, and the continuous typed opacity drag, shipped in this
  run (the scope section above); P7-27 moves to verified. Its request also
  named 8A/8E placement and typed `new_layer` at depth, which are SPLIT into
  12E-2b below rather than half-delivered. The original criteria, kept for
  the record: insert a
  live adjustment at ANY stack position (above the target, inside a group, at
  the top) at both depths; a continuous opacity/parameter drag previewing
  against the SAME captured target/selection the commit uses, producing ONE
  undo transition at gesture end, refusing stale results when the document
  revision changed under the gesture, and preserving neighbouring/group
  evaluation (isolated groups, clipping, Blend If, styles, the 11E
  linear-light policy); 8A/8E embedded and artwork placement extended so a
  placed layer can land at a chosen position (above the active layer, inside
  its group) rather than only as a top root, at both depths. Verification:
  an adjustment inserted at three positions at both depths with the render
  digest matching an independently composed expectation, a drag preview at
  three opacities compared to the committed render, ONE undo step after the
  gesture, a stale gesture refused after a concurrent edit, and a placed
  embedded image landing at the chosen position.
- **12E-3 — CLOSED for the BYTE half.** Knockout (Shallow/Deep) and
  `blend_interior_as_group` ship in the byte compositor with the design D1–D12
  above, the f64 oracle, byte `.rz` word 49, the typed/float/linear-light
  refusals and the D7 merge disclosure. The byte half is verified against the
  f64 ORACLE **and against Photoshop 26.10.0**: all ten P-cases RAN under
  D12's file-level gate and all ten are **MATCH** within T2 ("What 12E-3
  verified" carries the per-case numbers). **P7-37 stays OPEN for the TYPED
  half only** — the typed compositor, typed `.rz` word 50, PSD `knko`/`infx`
  and the P-cases re-measured at 16 bits under T3 are 12E-3b's, whose
  contract inherits the same gate and exit rules. The original criteria, kept
  for the record: the byte-side DESIGN first, recorded in this doc before code — the
  interior/knockout evaluation order relative to styles, Blend If and
  clipping, the group-boundary rule for Shallow vs Deep (Shallow knocks out
  to the enclosing group's backdrop, Deep to the Background layer),
  `blend_interior_as_group`, and a PSD carriage decision (the
  `lsct`/`lspf`-adjacent knockout byte and the `iOpa`/interior
  flags — read and write, or read with disclosed loss). Then the byte
  implementation against independent references (a hand-computed compositor
  oracle in the test, and Photoshop-rendered fixtures from
  `scripts/photoshop-probe/` where the machine has Photoshop 2025), then the
  typed one against the same references, with native persistence (LBLD or a
  new record, version bump) and PSD exchange agreeing with the qualified
  result; the 11E and 11F policies either migrate or refuse by name.
  Verification: byte and typed renders of Shallow and Deep knockout with and
  without `blend_interior_as_group` against the oracle and the Photoshop
  fixture within a fixed tolerance recorded BEFORE measuring, a PSD round trip
  and a native round trip. **Carried item 1 from 12D-1 — PSD `cmls` (layer
  comp) carriage — is 12E-3's**, since that run is the one that touches PSD
  exchange: import it if it is bounded (comps that reference layers by record
  index), else keep the disclosed loss and re-record the owner. **`cmls` is
  RE-OWNED by 12E-3c** with the rest of the PSD work.

- **12E-3b — CLOSED for what it shipped.** The TYPED twin of knockout /
  "Blend Interior Effects as Group" (D1–D8 at 16 bits, `precision_knockout.rs`
  under T3, float refusing by name and `linear_light` by the existing
  sentence), typed `.rz` word **50** nested inside 46 after 48 with no table
  and an identical reopen/resave, the Blending Options rows at both depths
  with their UI-SOURCE probe case, the `merge_visible` depth-floor sentence,
  D6's clip-base narrowing CLOSED by decision (ii), and contract (g) — Merge
  Down and Merge Layers evaluate knockout against the REAL backdrop at 8 and
  16 bits, Merge Visible stays exact, and the structural residual is disclosed
  on the `merge_down` reply (decision xi's R1–R3) instead of the removed
  blanket note. Verified by `core/tests/knockout_merge.rs`,
  `verify_phase12e3b_typed_knockout.py` (K1–K8), the re-pointed
  `verify_phase12e3_knockout.py` (G1–G8 with the negative control) and the
  9H probe's `knockout-blending-ui` / `knockout-merge-menu-ui` cases. **What
  it did NOT deliver is 12E-3c's, below** — PSD `knko`/`infx`, PSD `cmls`,
  D5's Background divergence and the 16-bit Photoshop re-measurement — so
  **P7-37 stays OPEN** and plan 12E is NOT "Shipped".
- **12E-3c — CLOSED.** `knko` / `infx` read AND written at both depths, D5's
  Background divergence DECIDED as D13 (a deliberate product decision,
  disclosed on import and export), E9's separation of `clbl` from `infx`, R4
  on the merge residual, and the C-gates of
  `scripts/verify_phase12e3c_psd.py` shipped in that run (the 12E-3c scope
  section above); the Photoshop block it re-owned — P16, PD, PX and the two
  contract observations — was MEASURED by **12E-3d** and is closed with it
  (the next item). The `cmls` evidence family and P7-38 remain 10C-3's and
  never held it open.
- **The 12E-3c Photoshop block — CLOSED by 12E-3d.** Every criterion below
  was measured, with its numbers in the "12E-3d scope" section above: step 9's
  `resave` derivation and `knockout_bottom` authoring both ran; **P16 is ten
  MATCH** at 16 bits under T3 with a non-vacuous control on the nine gated
  cases; **PD is DIVERGENCE (disclosed) at 8 and 16 bits** with all four of
  E5's conditions met; **PX is eight MATCH** per keyed record — the FOLDER
  record included — with **PX-PAD MATCH at both depths over two candidates
  each** (`DeepKnock` and `PassKnock`, 22-byte `luni`, `knko` and `iOpa` both
  after it); and **both contract observations are RECORDED at both depths**,
  (a) naming the measured cost of D6's closed clip-base narrowing and (b)
  Photoshop's own Merge Down beside Rasterize's exact one. **P7-37 closes
  with it.** The criteria, kept for the record, copied VERBATIM from 12E-3c's
  spec
  (`03-spec-final.md` E5, E6, E8 and Approach steps 9–11). **Accepted when:**
  (step 9) the committed `knockout/source.psd` is RE-SAVED at depth 16 through
  `scripts/photoshop-probe/`'s `resave` job with the ten `CASE_VISIBLE`
  renders → `source16.psd`, `<case>_16.png`, `blocks16.json`, and the
  `knockout_bottom` family (ordinary base `Bottom` — no `background` key —
  then `Mid`, `DeepKnock` deep fill 0, `ClipBase` with a colour overlay +
  `blend_interior`, `ClipMember` clipped) is authored at depths [8, 16]; (step
  10) **P16** — the file-level gate on `source16.psd` (every described layer's
  `knko`/`infx` body in `blocks16.json` equals the described value, per record
  index), opened as a 16-bit document with the import ASSERTED to have carried
  every described key (a failure there is exit 1, never a MISS), each case
  rendered and compared under **E8's T3** (max RGB ≤ 2 8-bit-equivalent codes,
  `blend_if_knockout` ≤ 4, both sides matted on white), then E8's negative
  control with the keys cleared (`CONTROL_MIN_CODES = 32` /
  `CONTROL_MIN_CODES_16 = 8224`; `interior_off` exempt); (step 11) **PD** —
  `deep_over_ordinary_bottom` at 8 and 16 bits under **E5**: a fourth status
  **DIVERGENCE (disclosed)**, the drive exiting 1 unless ALL FOUR of E5's
  conditions hold at that depth (file level: record 0 HAS a `-1` channel and
  its rectangle equals the canvas, `DeepKnock`'s `knko` is `02000000`, and the
  IMPORTED document's D5 holds; the HOLE is the pixels where `high.png`'s
  alpha is 255 and the OUTSIDE the pixels where it is 0, both renders matted
  on white; inside the hole Photoshop is white within T2 /
  T3, Rasterize equals `base.png` within 1 8-bit-equivalent code, and the two
  differ by ≥ the control floor; outside the hole the two agree within T2 /
  T3; and the CONTROL — `add_layer_mask(layer: 0, kind: "reveal_all")` on the
  imported document, after which the WHOLE frame matches Photoshop within T2 /
  T3, then `undo` and the pre-control render returns), an unpredicted result
  being a FAILURE and Photoshop unavailable NOT RUN; and **PX** — **E6**'s
  per-keyed-record export verification on the C-document's exports, at each
  depth: MATCH = the re-saved record's `knko`/`infx` body equals what
  Rasterize wrote AND Photoshop's render of the KEYED export differs from its
  render of the ALL-KEYS-CLEARED export by ≥ `CONTROL_MIN_CODES` (8) /
  `CONTROL_MIN_CODES_16` (16), both rendered with only
  `[Bottom, Mid, <the record's top-level owner>]` visible; MISS = either half
  fails, recorded with bodies and max/mean; NOT RUN = Photoshop unavailable;
  verdicts are PER RECORD, so a FOLDER-record miss is named separately from
  the raster ones, and `getd` is still attempted and kept as an observation.
  PX also names ONE PADDING check, so a framing failure is reported as itself
  rather than as an unexplained PX MISS: a `knko` written after an
  odd-halfword `luni` (a record whose name makes the `luni` body's length ≡ 2
  mod 4) survives Photoshop's re-save with its body intact, and the record
  walk over the re-saved file stays framed — every later block of that record
  still reads as itself. Finally, the TWO contract OBSERVATIONS travel with
  this block — both need Photoshop, and `interior_on_clip_base` needs the
  `knockout_bottom` family step 9 authors — and are never gates:
  (a) `interior_on_clip_base` is measured at both depths and its max/mean
  recorded; (b) Photoshop's own Merge Down of scene 1 is recorded — `author`
  `[Background(true), MidA, MidB, Top deep fill 0]` into the receipts dir,
  then the `merge_down` harness job, with before/after max/mean in Photoshop
  and against Rasterize's `merge_down` of the same imported file.
  The `resave` and `merge_down` harness jobs travel with this block. Its
  closure rule was that every gated P-case at both depths is MATCH, PD meets
  its four conditions, PX passes per keyed record, and both observations are
  recorded — **all four are met by 12E-3d, so P7-37 is VERIFIED and plan 12E
  is Shipped.**
- **The `cmls` EVIDENCE family — owner 10C-3.** Never a gate. **Accepted
  when:** the `comps` family is authored (three rasters, comps `VisOnly` and
  `All` with one offset and one opacity change), `comps.json` written
  (resource 1065 hex; per record index the `shmd` item keys and the `cmls`
  body hex — which needs `psd_records.py` to return `resource_bodies` for id
  1065 and each record's `shmd` items), and the measured structure recorded in
  the 12D and 12E status docs beside E7. The `comps` description key of
  `scripts/photoshop-probe/photoshop.jsx` travels with this item, not with
  12E-3d's block.
- **P7-38 — a true Background layer kind / flag. Open, owner 10C-3.** Added by
  12E-3c's D13, which KEPT the structural Background rule and DISCLOSED the
  divergence rather than closing it. **Accepted when:** PSD import SETS, PSD
  export WRITES (lowest record, no `-1` channel, flags bit 0, `lnsr` `bgnd`)
  and `.rz` PERSISTS (one conditional word nested inside 45 / 46, older
  readers refusing it by name, reopen → resave byte-identical) a per-layer
  Background mark that D5 reads when present and the structural rule when
  absent; `hasQualifyingBackground` reads it; and `deep_over_ordinary_bottom`
  is MATCH at both depths with Photoshop re-saving a Rasterize-written
  Background as a Background.
- **P7-39 — `blend_interior_as_group` on a clip base. Open, owner 10C-3.**
  Added by 12E-3d's observation (a), which MEASURED what 12E-3b's decision
  (ii) closed without evidence: on a clip base with visible clipped members
  Rasterize evaluates the flag OFF (D6's narrowing) while Photoshop evaluates
  it ON — Photoshop renders `Multiply(base, overlay)` with the backdrop
  visible, Rasterize the flat overlay colour, max 255 / mean 34.08 at 8 bits
  and the same picture at 16. The shipped behaviour is unchanged by this run
  and P7-37 is NOT reopened; this row owns the difference, exactly as P7-38
  owns D5's. **Accepted when:** D6's narrowing is lifted at BOTH depths so
  that `interior_on_clip_base` is MATCH within T2 at 8 bits and T3 at 16 bits
  against the committed `knockout_bottom` renders, with the f64 oracle
  extended to the clip-base case FIRST (the oracle leads the change, not the
  Photoshop render), and the native and PSD round trips unchanged.
- **PSD `cmls` import/export — owner 10C-3** (re-owned from 12E-3c by E7,
  which measured that the premise is unverified). **Accepted when:** from the
  committed `comps` family's measured structure, comp names, comments,
  capture flags and per-layer visibility import at both depths bound to the
  imported layers' ids (`LCMP` schema 2), position and appearance either
  carried from measured semantics or dropped with a per-comp loss sentence,
  stale/duplicate handling per `doc_comps.rs`, and a Photoshop-authored
  fixture test. Until then the import keeps no comp and the docs say so.
- **12E-2b — CLOSED.** Placement at a chosen position (embedded, linked,
  host media, artwork, text and shape), typed `new_layer` at 16 bits and at
  float, and the retained-source / attachment lock families at both depths
  shipped in this run (the scope section above). `stamp_visible` /
  `flatten_image` at depth are RE-OWNED by 12E-2c rather than
  half-delivered; its criterion is the next item. The original criteria,
  kept for the record. It OWNED: `position` / `layer` on
  `place_image` (embedded and linked), host-media placement and typed
  `add_text_layer` / `add_shape_layer`, all through 12E-2's
  `inserting_record` / `adding_layer_at` slot rule — which is already
  written and tested, so this is a call-site change and not a new rule;
  typed `new_layer` with the Layers panel's `addButton` enabled at 16 bits
  and at float, admitted BY NAME in `DocumentPrecision` and
  `FloatDocument.tools`; `under_locks` / `lock_refusal` on every
  `precision_retained_source.rs`, `doc_retained_source.rs` and
  `raw_source_edit.rs` writer at BOTH depths with byte tests; and
  `stamp_visible` / `flatten_image` at depth, delivered or re-owned —
  RE-OWNED by 12E-2c, whose criterion is recorded in full below.
  **Accepted when:** a placed embedded image lands at each of the three
  positions at 8 and 16 bits (and at float where the tool is admitted) with
  every neighbour's identity and allocation kept; `new_layer` at depth at
  three positions is ONE undo step and replays through `run_action`; a
  locked retained-source layer refuses Replace and Transform BY NAME at both
  depths; all of it in an owned drive
  `scripts/verify_phase12e2b_placement.py`; and the Phase 8 / 8E status
  docs' "typed artwork insertion appends a top root" rule is rewritten to
  the slot rule.
- **12E-2c — CLOSED.** `stamp_visible` / `flatten_image` at 16 bits and at
  float shipped in this run (the scope section above), and every criterion it
  was written against is met there, with ONE correction recorded rather than
  smoothed over: the criterion said the flatten's comps go "stale by name",
  and the BYTE op drops the whole list (`..from_pixels`), so the typed op
  empties it too — the byte rule wins, and the drive's C4 gate is written
  against the emptying. The dead `RzDocument::adding_image_layer_at` that
  12E-2b carried is DELETED with it (no caller, no export, no test; its two
  prose mentions are rewritten).
- **The 12E-1b menu whitelist gap — FOUND AND CLOSED in 12E-2c.** The eight
  selectors `mergeDown:`, `mergeVisible:`, `linkLayers:`, `unlinkLayers:`,
  `lockTransparency:`, `lockPixels:`, `lockPosition:` and `lockAll:` had
  working typed cores and first-line typed `RasterCore` routes since 12E-1b
  but were still disabled by `validatePrecisionItem`'s closed whitelists.
  All eight, plus 12E-2c's two, are in BOTH sets now after a per-selector
  check of the float route, and Merge Visible / Stamp Visible validation
  became DEPTH-AWARE (`contributingRasterLeaves()`) so an enabled item can
  never miss the typed CONTRIBUTING RASTER floor — that floor only: a
  capacity refusal from `preflight_raster_addition` or an unmigrated
  `linear_light` render refusal still reaches the host as nil and beeps.
  Measured in `stamp-flatten-menu-ui` at 16 bits and in the
  drive's C8 proxy at float. Not carried.
- **The `StructureEdit::MergeVisible` unit variant — CLOSED in 12E-2c, by
  owner decision.** It silently accepted extra keys despite
  `deny_unknown_fields` (`precision_geometry.rs`'s serde rule, the same defect
  `Flatten {}` was written to avoid). It is `MergeVisible {}` now, with the
  `raw_local_coverage.rs` comment, every match arm updated, and
  `core/tests/precision_stamp_flatten.rs` asserting that
  `{"op":"merge_visible","x":1}` refuses with the `invalid high-depth layer
  property` prefix beside the two 12E-2c ops. No other `StructureEdit` variant
  was a unit variant. Not carried.
- **The FLOAT selector whitelist has no UI-source gate — owner 12E-3.** The
  9H probe has no float opener, so `stamp-flatten-menu-ui` asserts its two
  enablement tables at 16 bits only; the drive's C8 float MCP proxy
  (`set_layer_lock`, `link_layers`, `unlink_layers`, `merge_down`,
  `merge_visible`, each non-error with one `undo`) is the evidence until then.
  **Accepted when:** the 9H probe gains a float opener (from
  `core/tests/fixtures/float_11f/scene.tiff`) and `stamp-flatten-menu-ui`
  asserts its (a) and (b) tables at float.
- **The MCP `merge_visible` no-op wording at depth — owner 12E-3.** A
  16-bit document whose visible stack holds one raster and one adjustment now
  correctly DISABLES the menu item, but the MCP tool still reaches the core's
  "requires two contributing raster layers" refusal without the host's own
  narrower explanation of why an adjustment layer does not count.
  **Accepted when:** the `merge_visible` handler answers that case with a
  sentence naming the depth-specific floor, tested in an owned drive.
- **Out of 12E-1 entirely, with existing owners:** typed "new layer" at depth
  (delivered by 12E-2b; the panel's `addButton` is enabled at every depth
  now), typed Paste (12G),
  multi-layer Free Transform (12F-2), and any change to comps' binding or to
  `ActionIdentity.session` beyond what 12E-1b names above.
