# Phase 11A — export workspace status

**11A ships the export workspace**: one pure
render → resample → sharpen → convert → reduce → encode chain at both depths,
an encoded preview whose bytes ARE the bytes written, a versioned
naming/destination preset library, and File ▸ Quick Export reachable from the
menu, MCP and Actions. The editable master, every existing export path and
every committed golden are untouched.

The plan's 11A text is "Add output dimensions, profile conversion, encoded
preview/file size, naming/destination presets and Quick Export. Integrate
output sharpening once 9B is available." All six are in; 9B's Unsharp Mask
kernel is the sharpening stage, generalized to u16 so a 16-bit deliverable is
never sharpened through a byte round trip.

## What ships

| Piece | Where | Note |
| --- | --- | --- |
| The pipeline | `core/src/doc_export.rs` | `RzDocument::export_encoded` (8-bit) and `PrecisionDocument::export_plan` / `export_encoded` (16 or 8 bit) |
| The FFI | `core/src/ffi_export.rs`, the header's "Export workspace" section | `rz_export_output_size`, `rz_export_expand_name`, `rz_doc_export_encode`, `rz_precision_doc_export_plan`, `rz_precision_doc_export_encode`, and the five `rz_export_result_*` calls |
| The generic sharpen | `core/src/ops_photographic.rs` | `unsharp_mask_samples<T: Unorm>`; the byte `unsharp_mask` is now a thin caller and its bytes are unchanged |
| The model | `app/Sources/ExportWorkspace.swift` | `ExportSettings`, `ExportPreset`, `ExportPresetLibrary`, `ExportCapture`, `ExportNaming`, `ImageDocument.quickExport` |
| The sheet | `app/Sources/ExportWorkspaceSheetController.swift`, `ExportPresetSheets.swift` | File ▸ Export… (⌘E) with the encoded preview; the two preset sheets |
| The commands | `app/Sources/EditorViewController+Export.swift`, `AppDelegate.swift` | File ▸ Export… ⌘E and File ▸ Quick Export ⌥⌘E |
| MCP parity | `app/Sources/AgentServer+Export.swift` | `export_preview`, `export_image`, `quick_export`, `list_export_presets`, `save_export_preset`, `delete_export_preset`, `set_default_export_preset` |

## The one request schema

The FFI, the preset file and every MCP tool speak the same object, parsed only
in Rust:

```json
{"format":"png|jpeg|tiff|bmp|gif|webp","jpeg_quality":1..100,"bit_depth":8|16,
 "dither":"none|ordered","size":{"mode":"none|width|height|long_edge|scale","value":number},
 "filter":"nearest|bilinear|catmull-rom|lanczos3","profile":"document|srgb|display-p3",
 "sharpen":{"radius":0..64,"amount":0..5,"threshold":0..1}|null,
 "embed_profile":bool,"strip_metadata":bool}
```

An unknown key refuses rather than being ignored: a misspelled `jpeg_qualtiy`
that quietly exported at 90 would only be discovered in the delivered file.

**With every optional field omitted the bytes are today's bytes.** That is a
test, not a claim: `core/tests/export_pipeline.rs` compares a default request
against `rz_doc_save_image` for all six byte formats and against
`rz_precision_doc_save_projection16` for BOTH PNG16 and TIFF16.

## Stage order, and why

Resample → sharpen → convert → reduce → encode.

* **Resample before sharpen.** Output sharpening is a deliberate correction for
  the resampling loss; sharpening first would throw most of the added detail
  away again. The test pins this both ways: the pipeline's bytes equal
  `rz_image_resize` → `rz_image_unsharp_mask` → `rz_doc_convert_to_profile` →
  `rz_doc_save_image` composed in that order, and DIFFER from the
  sharpen-before-resample composition.
* **Convert after sharpen.** The conversion is a per-pixel colour map; running
  it last among the pixel stages keeps the sharpen in the document's own space.
* **Reduce last**, so every earlier stage keeps the document's own precision. A
  16-bit master resamples and sharpens at 16 bits before any dither.

No new pixel math was written. Every stage routes through the shipped kernel:
`ops::resize` / `precision_sampling::resize_image`,
`ops_photographic::unsharp_mask_samples`,
`retained_source_render::color8` / `color16`,
`PrecisionImage::quantize_with_context`, and `rz_image::encode_into` /
`precision_codec::encode` behind the same `MetadataInjector` selection
`save_flat` and `precision_codec::save` use — including
`MetadataInjector::png(…, side.png_chunks())` for PNG16, without which the
`pHYs` and XMP chunks would silently vanish from a 16-bit PNG.

## The preview contract

`ExportResult` holds the encoded BYTES. `rz_export_result_preview` decodes
those bytes; `rz_export_result_write` writes those bytes. "The preview matches
the encoded output" is therefore an identity rather than a promise, and
`export_preview`'s `encoded_sha256` is the sha256 of the file `export_image`
writes.

## Decisions recorded

Owner-level decisions were delegated (`request.md`); each is made here with its
reasoning.

1. **Size modes: none / width / height / long edge / scale.** Aspect ratio is
   always kept — a delivery that changed the shape of a photograph would be a
   bug, and a crop is a document edit. Long edge is the one that makes a mixed
   portrait/landscape batch behave, so it is in even though width and height
   can express it per file. Percent scale is capped at 16×, a side at 30000 px
   and the pixel count at the document ceiling.
2. **Default kernel Lanczos3.** It is the sharpest of the four the core already
   has and the usual choice for downsampling a photograph; the other three stay
   available because a pixel-art or screenshot delivery wants Nearest.
3. **Profiles: document / sRGB / Display P3.** Exactly the RGB matrix/TRC
   conversions this build supports (`icc_builtin`'s two blobs). A LUT or
   non-matrix source profile refuses with the core's own sentence rather than
   being silently kept. Rendering intents, black-point compensation, proofing
   and gamut warnings are 11C, as the plan's boundary says.
4. **Sharpening off by default, 9B's own ranges** (radius 0–64 px, amount 0–5,
   threshold 0–1 normalized). A zero radius or a zero amount is an exact
   bypass, which is what the kernel already reports as "nothing changed".
5. **Presets: 64 × 4 KiB.** A quarter of the filter library's count because an
   export preset is a delivery target rather than a look, and half its byte cap
   because the payload is a handful of scalars plus two strings. Digest
   filenames, an advisory `flock`, atomic publication and whole-library
   strictness are the `LiveFilterPresetLibrary` contract unchanged.
6. **Tokens: `{name}` (required), `{width}`, `{height}`, `{format}`, `{date}`,
   `{preset}`; default pattern `{name}-{width}x{height}`.** `{name}` is
   mandatory because a pattern without it names every document in a folder the
   same thing — which is exactly how a batch silently overwrites its own
   output. The expansion must be a plain file name: no `/`, `\`, `:`, control
   character or leading dot, at most 200 characters, so a pattern can never
   escape the destination folder. The host supplies `{date}` as a local
   `YYYY-MM-DD` string, because a pure core has no calendar and no time zone.
7. **Collision = refuse.** `ExportResult::write` refuses an existing path, and
   Quick Export never passes `overwrite`. The sheet's Save panel is the ONE
   caller that passes `overwrite: true`, because the panel already asked. No
   auto-suffixing: a silently renamed deliverable is worse than a refusal.
8. **No seeded presets; Quick Export refuses until a default is set.** A
   destination folder is a choice only the person exporting can make, so
   shipping one would either point somewhere wrong or invent a folder. The
   default preset's name lives in a hidden `.default` file (≤ 512 bytes) beside
   the presets, so marking a different preset the default rewrites nothing a
   recorded Quick Export depends on.
9. **The destination folder must exist; it is never created.** Creating a
   folder from a preset is a filesystem side effect an export has no business
   having, and a typo in a preset would otherwise scatter directories.
10. **The sheet's successful write is the one UI writer of the five
    per-document save preferences.** Deleting `ImageDocument.exportDocument`
    and `ImageDocument.exportPrecisionDocument` removed the only places
    `jpegExportQuality`, `embedColorProfile`, `stripMetadata`, `exportBitDepth`
    and `exportDither` were set, and `CapturedSave` reads all five for every
    Save/Save As. The export sheet therefore writes them back exactly as those
    two functions did — no `updateChangeCount`, followed by
    `history.refreshCurrentViewState()` so Undo cannot revert the preference.
    MCP `export_image` / `quick_export` and File ▸ Quick Export never write
    them (`save_copy`'s precedent: read as defaults only). The three NEW
    options — size, profile, sharpening — are sheet state seeded from
    `ExportSettings.defaults(for:)` or a preset and are never stored on the
    document: they describe one delivery, not the picture.
11. **One `presentExportNotice` for every export outcome.** The dropped-packet
    message (`ExportCapabilities.droppedMessage`) is what the two deleted
    commands ended with; `RasterExportResult.saveReport()` provides the
    `RZ_CARRIES_*` input, and Quick Export appends the same message to its
    written-path notice.

## Actions

`quick_export` IS a step; `export_image` is not.

The `save_copy` / `export_psd` containment rule (`Action.swift`) exists because
a step's path is an ABSOLUTE one recorded when the action was written, so one
`save_copy` step run over a folder of 300 files wrote 300 times to the same
path. `quick_export` cannot do that: its name comes from a pattern that MUST
contain `{name}`, so every document in a batch produces a different file, and
it never overwrites — an existing path refuses. `export_image`, which takes a
literal path, is in `notAStepTool` for exactly the old reason, and its refusal
sentence names Quick Export as the thing to record instead.

The shared `ImageDocument.quickExport` records ONE resolved step carrying the
settings, destination and pattern, so replay never depends on a mutable library
name. The generic `recordAgentCall` must therefore not record it: all seven
tool names are in `ActionCatalogFacts.nonRecordable`.

## Verification

* `core/tests/export_pipeline.rs` — thirteen tests through the FFI: the
  `output_dimensions` cases and every refusal; the stage order both ways; the
  default-request byte equality at both depths and for both 16-bit formats; the
  convert-only TIFF16 equality against `rz_precision_doc_convert_to_profile` +
  `rz_precision_doc_export16`; a nearest 50 % + ordered 8-bit PNG decoded
  against an independent point-selection and Bayer formula written in the test;
  the u16 sharpen against its byte twin within one code; the naming grammar and
  its refusals; a pre-cancelled control at both depths; the overwrite rule; the
  preview's dimensions; the NULL sweep; and the typed plan's sharpen charge.
* `scripts/verify_phase11a_export.py` — the owned MCP drive (private
  `RZ_EXPORT_PRESETS_DIR`, dummy key, free port). Seven gates: both
  deliverables' dimensions/depth/size read back by `sips`; `sha256(file) ==
  export_preview.encoded_sha256`; the master's `get_document` /
  `get_precision` / `get_color_profile` / dirty flag unchanged; Quick Export's
  path and its refusal to overwrite; the preset library across a relaunch; a
  recorded Quick Export replaying on a renamed copy; and the catalog's tool
  count.
* `public-docs/.../reference/mcp-tools.md` was regenerated from a live
  `tools/list` and now lists the seven export tools plus entries that had
  accumulated since its last regeneration — including the five `assisted_*`
  tools, whose catalog entries are unconditional (only their handlers are gated
  behind `RZ_ASSISTED_ANNOTATION`), so the server already advertises them to
  every client. The stated count therefore moves 181 → 195; documenting what
  the server actually returns is the honest reading, and hiding the gate's
  tools would be the reference lying about `tools/list`.

## What stays open for 11B/11C

* Layer and group exports, contact sheets, metadata-policy presets and batch
  delivery — **11B**.
* Broader ICC transforms, rendering intents, black-point compensation, soft
  proofing and gamut warnings — **11C**. 11A deliberately ships with the
  current supported RGB conversion, which the plan's boundary allows.
* HEIC, PDF and the other format additions — **11D**.
* Auto-suffix collision handling, creating a destination folder, and seeding a
  starter preset are refused above with reasons rather than deferred.
* The Batch window keeps its own `ExportAccessoryController`; wiring Batch to
  export presets belongs with 11B's batch delivery work.
