# Phase 11B — production outputs status

**11B ships production outputs**: per-item layer and group exports, pure-Rust
contact sheets, a named metadata-policy library applied inside 11A's export
request, and preset-driven Batch delivery. Everything goes through 11A's
`ExportRequest` / `ExportResult` / preset / naming path rather than beside it,
and with the new options left out the bytes are 11A's bytes — the export
pipeline's default-request equalities, the `psd_10c` goldens and `save_image`
are all unchanged and still tested.

The plan's 11B text is "Add layer/group exports, contact sheets and reusable
metadata policies/presets. Extend batch delivery using the existing
Actions/Batch infrastructure." All four are in.

## What ships

| Piece | Where | Note |
| --- | --- | --- |
| Layer/group exports | `core/src/doc_export_layers.rs` | `top_level_items`, `isolated_root`, `alpha_bounds8`/`alpha_bounds16`, `crop16`, `export_layer_encoded` at both depths, `export_layer_plan` for the typed one |
| Contact sheets | `core/src/contact_sheet.rs` | `ContactSheetRequest` (parse, `dimensions`, `cell_origin`, `fitted`) and `compose` |
| Metadata policies | `core/src/export_metadata.rs`, `core/src/doc_color.rs`, `core/src/metadata_xmp.rs` | `MetadataPolicy` / `MetadataOutcome`, `image_sidecar_with_policy`, `xmp_stamped` / `xmp_minimal` |
| The request key | `core/src/doc_export.rs` | `"metadata"`, `export_stages` (stages 1–5, both depths), `NameTokens.layer` / `.index`, `ExportResult.info_json`'s `metadata` and `layer` |
| The FFI | `core/src/ffi_export.rs`, the header's "Production outputs (Phase 11B)" section | `rz_doc_export_layer_encode`, `rz_precision_doc_export_layer_plan`, `rz_precision_doc_export_layer_encode`, `rz_contact_sheet_size`, `rz_contact_sheet_compose`, `rz_metadata_policy_validate` |
| The models | `app/Sources/MetadataPolicy.swift`, `LayerExport.swift`, `ContactSheet.swift` | `MetadataPolicy`/`MetadataPolicyLibrary`, `LayerExportRequest`/`ImageDocument.exportLayers`, `ContactSheetLayout`/`ImageDocument.contactSheet` |
| The sheets | `app/Sources/LayerExportSheetController.swift`, `ContactSheetSheetController.swift`, `MetadataPolicySheets.swift` | File ▸ Export Layers…, File ▸ Contact Sheet…, and the Save/Manage pair |
| The commands | `app/Sources/EditorViewController+Export.swift`, `AppDelegate.swift` | File ▸ Export Layers… and File ▸ Contact Sheet…, after Quick Export |
| MCP parity | `app/Sources/AgentServer+ProductionOutputs.swift` | `export_layers`, `contact_sheet`, `contact_sheet_from_sources`, `list_metadata_policies`, `save_metadata_policy`, `delete_metadata_policy` |
| Batch delivery | `app/Sources/BatchRun.swift`, `BatchWindowController.swift` | "Deliver with:" an export preset and "Metadata policy:", writing through the `quick_export` tool |
| Tests | `core/tests/export_production.rs`, `core/tests/common/metadata_fixtures.rs` | 16 cases; the shared EXIF/IPTC fixtures moved out of `metadata_tests.rs` / `metadata_tiff_tests.rs` |
| The drive | `scripts/verify_phase11b_production.py` | Eight gates on a built Release app |

## The contract

### The metadata policy

The export request gains ONE key, parsed only in Rust:

```json
{"metadata": {"exif":"default|keep|drop", "xmp":…, "iptc":…,
              "density":"default|keep|drop",
              "stamp":{"author":string|null,"copyright":string|null}|null}}
```

`default` defers to `strip_metadata` (11A's behaviour); `keep` writes the
packet even when `strip_metadata` is set; `drop` never writes it. `density`
governs the container's own slot (the JFIF APP0, the PNG `pHYs`) and ignores
`strip_metadata`, so `keep` there is identical to `default`. Omitting the key
entirely produces 11A's bytes, which `export_production.rs` pins directly
(default-policy bytes == no-`metadata` bytes, on both JPEG and PNG).

`rz_export_result_info` always carries `"metadata": {profile, exif, xmp, iptc,
density, stamped}` — what the encode ACTUALLY wrote — and a layer result also
carries `"layer": {index, name, origin, size}`.

### Two validate-time refusals

Both are checked when the request is parsed, because each would otherwise let
the tool report something the bytes contradict:

* **A stamp with nowhere to go.** The stamp is written into the XMP packet as
  `dc:creator` / `dc:rights`; if XMP resolves to dropped, or the format cannot
  carry XMP, the stamp would be a silent no-op. Refused with "a metadata stamp
  is written into the xmp packet, which this export does not write".
* **`density: drop` beside a written EXIF or XMP packet.** A kept EXIF packet
  restates the ppi in IFD0 — where Exif/DCF makes it the AUTHORITY over a JFIF
  or `pHYs` density — and a kept XMP packet restates it again as
  `tiff:XResolution`. Refused with "density: drop requires the exif and xmp
  packets to be dropped too; each carries its own copy of the ppi".

### Naming tokens

`{layer}` and `{index}` join the 11A set. `{layer}` is sanitized (`/`, `\`,
`:` and control characters become `-`, the result is trimmed, an empty result
becomes the literal `layer`); `{index}` is 1-based and zero-padded to `count`'s
digit width (3 of 12 → `03`). A pattern using either token in an export that
has no item REFUSES rather than expanding to nothing, because a token that
silently vanished would name every item in a batch the same thing.

## Recorded decisions

Every decision below was delegated (`request.md`: trim rule, grid defaults,
packet set, policy bounds, batch semantics) and is recorded here with its
reasoning.

### Orientation is not a policy choice

The design started with an `orientation: keep|reset` rule and it was removed.
`rz_image::open_bytes` bakes the camera's rotation INTO the pixels, so a packet
that kept `Orientation: 6` would rotate the delivered picture a SECOND time in
every viewer — the exact bug `metadata_xmp.rs` exists to prevent. Both
normalizers therefore keep resetting it unconditionally and keep their
signatures; there is no `orientation` key, no `MetadataOutcome.orientation_reset`
and no `reset_orientation` parameter anywhere. The Rust test and the drive both
assert Orientation 1 in every written EXIF and XMP packet.

### The trim rule, and when a transparent item refuses

An item is a **depth-0 entry** — a raster layer or a whole group with its
members. Nested entries are deliberately not exported individually: a layer
inside a group is part of that group's picture. Items are numbered in PANEL
order (top first), which is what `{index}` counts and what the report shows.

`trimmed: false` delivers the CANVAS, so every item of one document lines up
pixel for pixel; `trimmed: true` delivers the item's alpha bounding box
(alpha > 0), with the rectangle reported as `origin` and `size`.

**A fully transparent item is a refusal only when trimmed.** There is no
rectangle to deliver, so trimming it would have to invent one. Canvas-framed it
exports as a fully transparent frame, which is exactly what a contact-sheet
cell needs — an empty layer becomes a legible background-coloured cell rather
than failing the whole sheet.

### The isolated render

`isolated_root` builds a document holding ONLY that item's subtree and
flattens it through the shipped compositor. Consequences, all deliberate:

* The item's opacity, blend mode, mask, style and Blend If apply against
  TRANSPARENCY. A Multiply layer delivers its own pixels rather than the black
  its blend against a white backdrop would produce.
* The root's `visible` flag is forced true — the caller asked for THIS item, and
  a hidden item delivering an empty frame would be a silent failure.
  Descendants keep their own visibility, because a hidden member of a group is
  part of how that group looks.
* A clipped top-level item composites UNCLIPPED: its base is a different item
  and is not exported with it, so the existing "a clipped entry with no
  unclipped sibling below composites as if unclipped" rule applies unchanged.
* `selected` means the top-level ANCESTORS of the selected entries, de-duplicated.

### Contact-sheet geometry and colour

```
rows   = ceil(count / columns)
width  = gutter + columns * (cell_width + gutter)
height = gutter + rows    * (cell_height + caption_height + gutter)
```

The gutter is drawn outside the grid as well as between cells, and the caption
strip belongs to its cell rather than to the gutter — a caption pushed into the
gutter would sit as close to the picture below it as to its own. Defaults are
4 columns, 256 × 256 cells, a 16 px gutter, `#ffffff`, lanczos3. Caps: 1…256
cells, side ≤ 30000, pixels ≤ `MAX_PIXELS`.

`fitted` keeps the aspect ratio and NEVER upscales, so a sheet of 64 px icons
shows 64 px icons rather than a wall of blur; cells are centred, so a mixed
portrait/landscape sheet lines up on centres.

**Cells and the sheet are 8-bit sRGB, 72 ppi, no packets.** A contact sheet is
a proof — something to look at, print small and throw away — so there is
nothing for the extra depth to protect, and one destination space means a
Display P3 source and an sRGB source are shown honestly side by side. An
`export` output asking for `bit_depth: 16` is refused with
`ContactSheet.bitDepthRefusal` BEFORE any cell is built, rather than reaching
the byte encoder's misleading "convert it to 16-bit first".

**Captions are host-rendered bitmaps.** The core places and clips them and
rasterizes no text: type is a platform concern, and a core that grew a glyph
rasterizer for a file-name strip would be a much larger thing than a contact
sheet. Swift renders `DS.sans(11)` through `Bitmap.renderStraightRGBA(appKit:
true, space: sRGB)`, black on a light background and white on a dark one by
Rec.709 luma.

### Policy bounds

The library is `ExportPresetLibrary`'s contract exactly: 64 policies × 4 KiB,
`RZ_METADATA_POLICIES_DIR` (else `Rasterize/Metadata Policies`), digest
filenames, an advisory cross-instance `flock`, atomic publication and
whole-library strictness. A stamp string is at most 512 characters, carries no
control character, and at least one of `author` / `copyright` must be present.
Library writes are PREFERENCES: outside document Undo and never Action steps.

`ExportPreset` moves to schema 2 to carry an optional `metadata_policy` NAME.
A preset that names no policy is still WRITTEN at schema 1, so a library built
before 11B is byte-identical and an older reader is unaffected; the decoder
accepts 1 (no key) and 2 (optional key). The name is resolved at delivery time
by `resolvedSettings()`, so editing a policy changes what a preset delivers and
deleting one is a refusal rather than a silent fallback. A RECORDED step never
carries the name — it carries the resolved policy, the `quick_export`
precedent.

### Batch semantics

"Deliver with:" chooses an 11A export preset. In preset mode:

* the destination folder, the naming pattern and every delivery option come
  from the PRESET, so the pane's output folder, name template and overwrite
  controls do not apply and are dimmed;
* overwrite is forced OFF, because Quick Export never overwrites;
* the three flat-mode pre-checks (containment, the claimed set, the exists
  test) are skipped: the name is expanded by the core AFTER the encode (it may
  use `{width}`/`{height}`), so there is nothing to check yet, and Quick
  Export's own refusals replace them;
* the write goes through the `quick_export` TOOL with `index`/`count`, so the
  naming, the refusals and the recorded-step rule are not re-implemented.

Flat (`save_copy`) mode is untouched, including its bytes: `save_copy` has no
policy argument, so the "Metadata policy:" popup is enabled only for a preset
run.

**Batch still has no MCP twin** (the unchanged 11A/Actions decision): it
performs no edit of its own and is `open_document` + `run_action` +
`quick_export` in a loop, which an agent already has. The drive exercises
exactly that loop (P5). The Batch WINDOW's preset path is a manual runbook,
below.

### What is recordable

`export_layers` and `contact_sheet` ARE Action steps: both name a document and
a pattern-derived output, and both record ONE resolved step from the shared
`ImageDocument` method (so the generic agent-call recorder must not add a
second copy — they are in `nonRecordable`). `contact_sheet_from_sources` is
`notAStepTool`: a sheet built from files or other open documents has no single
document to replay against. `save_metadata_policy` / `delete_metadata_policy`
are `notAStepTool` for the preset library's reason: a recorded export carries
the RESOLVED policy.

## Verification

```
cd core && cargo fmt --check && cd ..
make lint
cd core && cargo test --release --test export_production
make test
make typecheck
make app
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase11b_production.py --output ~/devel/rasterize-fixtures/phase11-receipts/11b/<stamp>
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase11a_export.py --output ~/devel/rasterize-fixtures/phase11-receipts/11b/11a-regression-<stamp>
```

`core/tests/export_production.rs` covers the isolated render against a
`common::ref_composite` oracle written in the test, the trim rectangle and its
`origin`/`size`, the transparent item at both depths (refused trimmed, a
fully-alpha-0 frame canvas-framed, the 16-bit case decoded with
`precision_codec::decode`), `{layer}` sanitizing and `{index}` padding with
their absent-token refusals, the contact-sheet formulas and exact pixel
placement (a solid cell, a never-upscaled centred cell, a transparent source's
cell equal to the background, caption placement, the caps and the 0-cell
refusal), every packet rule on a JPEG and a PNG with the segments and chunks
parsed IN the test, Orientation 1 in every written packet, the valid
`density: drop` and its refusal, the stamp on a stampable and an unstampable
packet plus `xmp_minimal`, the `metadata` report against the parsed bytes,
default-policy byte identity, pre-cancelled controls on both new encoders and
on `compose`, and the NULL sweep of all six exports.

### The independent decoder

`exiftool` is absent on this machine, so the drive ships a bounded stdlib
parser: JPEG segments (APP0 density, APP1 EXIF IFD0 Orientation and
XResolution, APP1 XMP, APP13) and PNG `pHYs` / `eXIf` / `iTXt`, plus its own
8-bit PNG decoder for the contact-sheet pixel checks. It uses `exiftool`
additionally when `shutil.which` finds it, and never relies on it. The fixture
is a PNG the drive writes itself carrying `eXIf` (Orientation 6, 300 ppi), a
`pHYs` and an `iTXt` XMP packet with `tiff:Orientation='6'` and an
`rdf:Description` to stamp. IPTC is covered by the Rust test only, because the
fixture format carries none.

### Receipts

Receipts live under `~/devel/rasterize-fixtures/phase11-receipts/11b/`, never
in `/private/tmp`. The run that shipped this slice left
`20260917-164347/result.json` (all eight 11B gates true) and
`11a-regression-20260917-164355/result.json` (all seven 11A gates true); the
two earlier folders beside them are the two failed attempts that found
`new_layer`'s and `export_layers`' 16-bit availability, kept because a failed
gate IS a receipt.

## Manual runbook — the Batch window's preset path

The Batch WINDOW is modal AppKit with no MCP twin, so its preset path is
verified by hand rather than by the drive (which exercises the same delivery
through `open_document` → `run_action` → `quick_export`). This is a deferrable,
self-contained runbook; nothing else waits on it.

1. Save an export preset with a destination folder and a pattern containing
   `{index}` (File ▸ Export… ▸ Save Preset…).
2. Save a metadata policy (File ▸ Export… ▸ Metadata policy ▸ Manage… ▸
   Save As…).
3. Open File ▸ Automate ▸ Batch…, choose an action and an input folder of three
   images, set "Deliver with:" to the preset and "Metadata policy:" to the
   policy.
4. Expect: the output folder row, the name template and the overwrite checkbox
   have no effect (the template field is dimmed); the run writes three files
   into the PRESET's folder, numbered 1…3 by `{index}`; a second run refuses
   every file as already existing and writes nothing.

## What stays open

* **11C** owns rendering intents, black-point compensation, soft proofing and
  gamut warnings. A contact sheet's sRGB conversion is the shipped
  matrix/TRC one and states no intent.
* **11D** owns HEIC, PDF and the other formats, so a deliverable is still one
  of the six 11A formats.
* **12E** owns per-layer PPI/EXIF editing. 11B writes packets; it does not
  author them, and it inserts no XMP property other than `dc:creator` and
  `dc:rights`.
* **12B** owns batch qualification (mixed sizes, partial failures,
  cancellation, collisions). 11B adds the preset delivery mode; the flat mode's
  behaviour and bytes are unchanged.
* Not in this slice, by decision: exporting nested (non-top-level) layers
  individually, per-item size/profile overrides, auto-suffixing collisions,
  creating destination folders, 16-bit contact sheets, IPTC authoring, EXIF
  `Artist`/`Copyright` tags, a configurable orientation, stripping resolution
  tags out of a KEPT packet, seeded policies or presets, and an MCP `run_batch`
  tool.

11B owns no carryover row.
