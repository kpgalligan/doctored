# Production outputs — layer exports, contact sheets, metadata policies, batch delivery (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 11B, shipped 2026-09-17
- **Original:** [archive/phase-11b-production-outputs-status.md](../archive/phase-11b-production-outputs-status.md) — full history, measurements, receipts
- **Key commits:** `a88b2bb`

## Context
The plan asked for layer/group exports, contact sheets, reusable metadata policies and preset-driven batch
delivery on the existing Actions/Batch infrastructure. Everything had to ride [11A](phase-11a-export-workspace-status.md)'s
`ExportRequest` / `ExportResult` / preset / naming path, and with the new options omitted the bytes had to stay
11A's bytes (export-pipeline equalities, `psd_10c` goldens, `save_image`).

## Decisions
- **Code map.** `core/src/doc_export_layers.rs` (`top_level_items`, `isolated_root`, `alpha_bounds8/16`,
  `export_layer_encoded`, `export_layer_plan`), `core/src/contact_sheet.rs` (`ContactSheetRequest`, `compose`),
  `core/src/export_metadata.rs` (`MetadataPolicy` / `MetadataOutcome`) with `doc_color.rs` / `metadata_xmp.rs`.
  FFI: `dr_doc_export_layer_encode`, `dr_precision_doc_export_layer_plan/_encode`, `dr_contact_sheet_size`,
  `dr_contact_sheet_compose`, `dr_metadata_policy_validate`. Swift: `MetadataPolicy.swift`, `LayerExport.swift`,
  `ContactSheet.swift`, `AgentServer+ProductionOutputs.swift`.
- **One new request key, `metadata`**: per-packet `exif` / `xmp` / `iptc` / `density` ∈ `default|keep|drop`, plus
  `stamp {author, copyright}`. `default` defers to `strip_metadata`; `density` ignores `strip_metadata`. Omitting
  the key reproduces 11A bytes (tested). `dr_export_result_info` always reports what was actually written
  (`metadata {profile, exif, xmp, iptc, density, stamped}`, and `layer {index, name, origin, size}` for items).
- **Validate-time refusals** (each would let the report contradict the bytes): a stamp when XMP is dropped or the
  format can't carry XMP; `density: drop` while EXIF or XMP is kept (each restates the ppi, EXIF authoritatively).
- **No orientation policy.** `dr_image::open_bytes` bakes rotation into pixels, so every written EXIF/XMP packet
  says Orientation 1; keeping `Orientation: 6` would double-rotate. There is no `orientation` key anywhere.
- **Items are depth-0 entries** (a raster layer or a whole group), numbered in panel order (top first). Nested
  layers are part of their group's picture. `selected` means the top-level ancestors of the selection.
- **Isolated render** (`isolated_root`): the item's subtree alone, flattened through the shipped compositor against
  transparency (a Multiply layer delivers its own pixels); the root's `visible` is forced true, descendants keep
  theirs; a clipped top-level item composites unclipped (its base isn't exported with it).
- **Framing:** `trimmed: false` delivers the canvas (items align); `trimmed: true` the alpha>0 bounding box. A fully
  transparent item refuses **only when trimmed**; canvas-framed it is a transparent frame (a legible contact cell).
- **Tokens** `{layer}` (sanitized; empty → `layer`) and `{index}` (1-based, zero-padded to the count's width). Using
  either with no item **refuses** rather than expanding to nothing. A layer export's pattern needs `{layer}` or
  `{index}`; `{name}` is optional there and stays mandatory everywhere else.
- **Contact sheet geometry:** `width = gutter + cols·(cell_w + gutter)`, `height = gutter + rows·(cell_h +
  caption_h + gutter)`; the caption belongs to its cell. Defaults 4 cols, 256², 16 px gutter, white, lanczos3.
  Caps 1…256 cells, side ≤ 30000, pixels ≤ `MAX_PIXELS`. Cells fit aspect, **never upscale**, centred.
- **Contact sheets are 8-bit sRGB, 72 ppi, no packets** (a proof; one destination space shows P3 and sRGB sources
  honestly). `bit_depth: 16` refuses (`ContactSheet.bitDepthRefusal`) before any cell is built.
- **Captions are host-rendered bitmaps**; the core places/clips but rasterizes no text.
- **Policy library** = the export-preset contract: 64 × 4 KiB, `DR_METADATA_POLICIES_DIR`, digest filenames,
  `flock`, atomic publication; stamp strings ≤ 512 chars, no control chars, at least one field. Library writes are
  preferences — outside Undo, never Action steps.
- **`ExportPreset` schema 2** adds an optional `metadata_policy` name, but a preset without one is still written as
  schema 1 (old libraries byte-identical). The name resolves at delivery (`resolvedSettings()`); a deleted policy
  refuses. Recorded steps carry the **resolved** policy, never the name.
- **Batch "Deliver with:" preset mode** (`BatchRun.swift`, `BatchWindowController.swift`): destination, pattern
  and options come from the preset; overwrite forced off; the flat-mode containment and claimed-set checks do not
  apply, but the exact name is pre-checked BEFORE the encode through `ImageDocument.quickExportDestination` (THE
  Quick Export naming), so an existing file is a `skipped` row, never a halt; a missing preset folder is refused
  at submission (window and `run_batch`), and the window's Output row is dimmed and reads the preset's folder.
  The write goes through the `quick_export` tool with `index`/`count`. Flat `save_copy` mode unchanged.
- **Recordability:** `export_layers` and `contact_sheet` are steps (one resolved step each, so both are
  `nonRecordable` for the generic recorder); `contact_sheet_from_sources`, `save_metadata_policy`,
  `delete_metadata_policy` are `notAStepTool`.

## Rejected alternatives
- An `orientation: keep|reset` rule — keeping a rotation tag double-rotates pixels already baked.
- A glyph rasterizer in the core for captions — type is a platform concern.
- 16-bit contact sheets — nothing for extra depth to protect in a proof.
- Exporting nested layers individually, per-item size/profile overrides, stripping resolution tags from a kept
  packet, seeded policies — out by decision. (IPTC authoring and EXIF `Artist`/`Copyright` were out here too and
  were later built per document as File ▸ File Info…,
  [phase-3-color-metadata-status.md](phase-3-color-metadata-status.md); a policy's stamp over an authored credit
  replaces it rather than adding a second element.)
- An MCP `run_batch` in 11B — later reversed: 12B-1 added headless `run_batch` (`AgentServer+Batch.swift`).

## Consequences
New export features must keep default-policy byte identity and the `metadata` report truthful against the bytes.
11B inserts only `dc:creator` / `dc:rights` into XMP; it writes packets, it does not author them. Tests:
`core/tests/export_production.rs`; drive: `scripts/verify_phase11b_production.py`.

## Open items (verified 2026-09-26)
- `DONE` — a preset-mode collision is SKIPPED (not a failure that halts the run), the Batch window's Output row
  is dimmed and reads the preset's folder, and a missing preset folder is refused before the run starts, in the
  window and in `run_batch`. Gated by the 12B drive's B4 (`{index}-{name}` + a stamp policy: three files, a
  stop-on-error re-run skips all three with unchanged digests, a partial re-run writes two and skips one, a
  missing folder returns no `batch_id`).
- `DROPPED` — the maintainer's hand check of the Batch **window** preset path (preset with `{index}` + a policy,
  three inputs; expect the pane's folder/template/overwrite controls inert, three numbered files, a second run
  skipping all three). Only the modal window's UI remains unchecked.
- `DONE` — 11C intents/proofing (`54d2547`); 11D HEIC deliveries incl. contact sheets and layer exports (`0e1c496`).
- `DONE` — per-layer/document PPI editing (P7-26 verified in 12E; `set_resolution`).
- `DONE` — batch qualification (mixed sizes, partial failures, cancellation, collisions) in 12B-1
  (plan §12 "12B-1 … Shipped").
- `DROPPED` — the "not in this slice, by decision" list above.
- `DONE` — the contact sheet admits each layer encode and core JPEG/PNG file open before it runs, and its compose
  (`ContactSheetAdmission.composeScratch`, measured; record beside it) before running it as a cancellable job;
  the MCP reply reports `working_memory_plan_bytes`. Drive: `scripts/contact-sheet-admission/measure.py` (Release).
- `DROPPED` (2026-10-30) — moving contact-sheet cells off main and charging a 16-bit document cell's display
  projection: the open document already holds that projection, and AppKit text needs main.
