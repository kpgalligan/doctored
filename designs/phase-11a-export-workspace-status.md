# Export workspace — one pure delivery chain, preview = bytes (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 11A, shipped 2026-09-17 (before 11B)
- **Original:** [archive/phase-11a-export-workspace-status.md](../archive/phase-11a-export-workspace-status.md) — full history, measurements, receipts
- **Key commits:** `922db4a`

## Context
The plan asked for output dimensions, profile conversion, an encoded preview with file size, naming/destination
presets and Quick Export, with output sharpening once 9B existed. The editable master, every existing export path
and every committed golden had to stay untouched, and a 16-bit master must never be pushed through a byte round
trip on its way out.

## Decisions
- **One pipeline in the core**: `core/src/doc_export.rs` — `DrDocument::export_encoded` (8-bit) and
  `PrecisionDocument::export_plan` / `export_encoded` (8 or 16). FFI in `core/src/ffi_export.rs`:
  `dr_export_output_size`, `dr_export_expand_name`, `dr_doc_export_encode`, `dr_precision_doc_export_plan`,
  `dr_precision_doc_export_encode`, plus the `dr_export_result_*` family. *Why:* every surface speaks one request.
- **Stage order: resample → sharpen → convert → reduce → encode.** Sharpening corrects resampling loss (sharpening
  first throws it away); converting after sharpening keeps sharpening in the document's space; reducing last keeps
  document precision through every earlier stage. A test pins the order both ways.
- **No new pixel math.** Stages route through `ops::resize` / `precision_sampling::resize_image`,
  `ops_photographic::unsharp_mask_samples<T: Unorm>` (the byte `unsharp_mask` is now a thin caller, bytes
  unchanged), `retained_source_render::color8/color16`, `PrecisionImage::quantize_with_context`, and the same
  `MetadataInjector` selection `save_flat` / `precision_codec::save` use (PNG16 needs `side.png_chunks()` or
  `pHYs`/XMP vanish).
- **One request schema, parsed only in Rust** (`format`, `jpeg_quality`, `bit_depth`, `dither`, `size`, `filter`,
  `profile`, `sharpen`, `embed_profile`, `strip_metadata`). **Unknown keys refuse** — a misspelled key that silently
  defaulted would only surface in the delivered file.
- **Defaults are today's bytes** — tested: a default request equals `dr_doc_save_image` for all six byte formats
  and `dr_precision_doc_save_projection16` for PNG16 and TIFF16 (`core/tests/export_pipeline.rs`).
- **Preview is an identity**: `ExportResult` holds the encoded bytes; `dr_export_result_preview` decodes them and
  `dr_export_result_write` writes them, so `export_preview.encoded_sha256` is the written file's hash.
- **Size modes** none / width / height / long_edge / scale; aspect always kept (a crop is a document edit). Caps:
  scale ≤ 16×, side ≤ the format's own cap (65535 JPEG/GIF, 16384 WebP, 130048 HEIC, measured as ImageIO's encoder limit; none for PNG/TIFF/BMP), pixels ≤ the document ceiling. **Default kernel Lanczos3.**
- **Profiles** document / sRGB / Display P3 only at 11A (widened by [11C](phase-11c-output-color-status.md)).
- **Sharpen** off by default, 9B's ranges (radius 0–64, amount 0–5, threshold 0–1); zero radius/amount is an exact
  bypass.
- **Preset library** 64 × 4 KiB, `DR_EXPORT_PRESETS_DIR`, digest filenames, advisory `flock`, atomic publication
  (`ExportPreset` / `ExportPresetLibrary` in `app/Sources/ExportWorkspace.swift`). *(Whole-library strictness was
  later relaxed to per-entry `broken` rows by 12C-1's `PreferenceLibrary.swift`.)*
- **Naming tokens** `{name}` (required), `{width}`, `{height}`, `{format}`, `{date}` (host supplies local
  `YYYY-MM-DD`; the core has no clock), `{preset}`; default `{name}-{width}x{height}`. `{name}` is mandatory so a
  batch cannot overwrite itself; the expansion must be a plain file name (no `/ \ :`, control chars or leading dot,
  ≤ 200 chars) so it cannot escape the folder.
- **Collision = refuse, except the interactive Quick Export.** `ExportResult::write` refuses an existing path;
  only the sheet's Save panel (which already asked) passes `overwrite: true`. File ▸ Quick Export and an MCP
  `quick_export` WITHOUT `settings` (a preset name, or no arguments) write a taken name as `-2`, `-3`, … up to `-999`
  (`QuickExportNaming.firstFree`, `QuickExportCollision.suffix`), and the tool result adds `requested_path`. A call
  WITH `settings` + `destination` + `pattern` — the form every recorded step and batch delivery uses — still
  refuses, so replay and batch never write a second copy. Nothing is ever overwritten.
- **No seeded presets; with no default, Quick Export uses a built-in one**
  (`app/Sources/QuickExportDefaults.swift`): a PNG named `{name}` beside the document's file, or in `~/Downloads`
  for a document with no file. It is built per call and never enters the library. The default's name lives in a
  hidden `.default` file (≤ 512 B). **Destination folders are never created.**
- **The export sheet is the one UI writer of the five per-document save preferences** (`jpegExportQuality`,
  `embedColorProfile`, `stripMetadata`, `exportBitDepth`, `exportDither`) that `CapturedSave` reads — written
  without `updateChangeCount`, then `history.refreshCurrentViewState()` so Undo cannot revert them. MCP
  `export_image` / `quick_export` and File ▸ Quick Export never write them. Size/profile/sharpen are never stored on
  the document.
- **Actions:** `quick_export` IS a step (pattern-derived name, never overwrites); `export_image` (literal path) is
  `notAStepTool` like `save_copy`. `ImageDocument.quickExport` records ONE resolved step (settings, destination,
  pattern — never a mutable library name); all seven export tools are in `ActionCatalogFacts.nonRecordable`.
- Surfaces: File ▸ Export… ⌘E (`ExportWorkspaceSheetController.swift`), File ▸ Quick Export ⌥⌘E; MCP
  (`AgentServer+Export.swift`): `export_preview`, `export_image`, `quick_export`, `list_export_presets`,
  `save_export_preset`, `delete_export_preset`, `set_default_export_preset`.

## Rejected alternatives
- Auto-suffixing a replayed, batched or `settings`-form collision — silently renames a deliverable. (The
  interactive form suffixes, and says which name was taken.)
- Creating the destination folder — a filesystem side effect; a typo scatters directories.
- Shipping a seeded default preset in the library — would point somewhere wrong or invent a folder. (The
  built-in fallback is not a library entry and writes only into a folder that exists.)
- Sharpening before resampling — discards the correction.

## Consequences
Every later output feature (11B layers/contact sheets/metadata, 11C intents, 11D HEIC/PDF, 11F float) extends this
request and result rather than adding a parallel path, and must keep the default-request byte equalities in
`export_pipeline.rs` passing. Verification drive: `scripts/verify_phase11a_export.py`.

## Open items (verified 2026-09-26)
- `DONE` — layer/group exports, contact sheets, metadata policies, batch delivery (11B, `a88b2bb`).
- `DONE` — intents/BPC/wider ICC (11C, `54d2547`); HEIC/PDF formats (11D, `0e1c496`).
- `DONE` — Batch wired to export presets ("Deliver with:", `app/Sources/BatchRun.swift:52`, `a88b2bb`).
- `DONE` — Quick Export works with no setup (the built-in PNG preset) and suffixes a taken name in its
  interactive form; drive gates E4, E4b, E4c. Out of scope there: Contact Sheet options, Export As canvas size
  and scale, nested Layers to Files, Slices.
- `DROPPED` — folder creation, seeded library presets, a suffix on a replayed or batched export (refused by
  decision above).
