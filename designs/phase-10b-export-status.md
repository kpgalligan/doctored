# Basic layered PSD export (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10B, phase 1 (writer) then phase 2 (GIMP reopen); closed, `styled_effects` case re-run 2026-09-16
- **Original:** [archive/phase-10b-export-status.md](../archive/phase-10b-export-status.md) — full history, measurements, receipts
- **Key commits:** `01a9541` (writer, preflight, UI, MCP), `4f793bd` (GIMP qualification, close), `fc70079` (styles written as `lfx2`)
- **Photoshop harness:** runners under `scripts/photoshop-probe/` and the `verify_*photoshop*` drives live in the private archive `kpgalligan/doctored-dev`.

## Context
Doctored could read PSD ([phase-10a-qualification.md](phase-10a-qualification.md)) but not write it.
10B adds a classic RGB PSD writer for the editable stack or a flattened composite. The governing rule is
**"never silent"**: everything the format cannot hold, or that Doctored's own importer will not bring back,
is refused with a reason before a byte is written or listed in the report both the user and the model see.

## Decisions
- **Written contract** (`core/src/psd_export.rs`, `psd_export_encode.rs`, `psd_export_preflight.rs`):
  - Header mode 3, four channels with merged alpha declared; 8-bit from 8-bit documents (16-bit added by
    [phase-10c-depth-status.md](phase-10c-depth-status.md) — depth always follows the document).
  - Raster layers: one record, four PackBits planes (`-1`, `0`, `1`, `2`), straight samples, exclusive
    signed bounds — hidden RGB under alpha 0 and off-canvas rectangles round-trip byte-exact.
  - Masks: ordinary **absolute** 20-byte body + a `-2` PackBits plane; outside default 0/255 and disabled
    flag. Density, feather and PSD link state are not written.
  - Groups: `lsct` 1/2 (open/closed) folder records with an `lsct` 3 closing divider and four empty channels.
  - Opacity byte `round(opacity*255)`; visibility flag bit 1; clipping byte; blend keys via
    `psd_group_records::PSD_BLEND_KEYS` read backwards, Pass Through on folders.
  - Names: truncated Pascal name + full UTF-16BE name in `8BIM luni`.
  - ICC: resource 1039 byte-identical; PPI: resource 1005 (16.16).
  - Composite preview: PackBits, RGB **matted against white** — alpha exact; color exact at alpha 0/255,
    within `ceil(127.5/a)` codes elsewhere.
- **Refused** (mode unavailable, destination untouched): side > 30,000 px (canvas, layer or mask);
  worst-case file > 2 GiB; masks with density or feather; a group whose style would need a bake. (At
  10B, all adjustments, all styles and all 16-bit documents refused; those were narrowed by 10C — see
  Open items.)
- **Reported, not refused:** group clipping (Photoshop reads a clipped group unclipped, [#2](https://github.com/kpgalligan/doctored/issues/2)); exceeding Doctored's own
  layered-reopen envelope (the working limit) (`psd_layer_pixels::peak_forecast` evaluated on the file being written) is a
  loss saying it will reopen *here* as a composite only — a file Photoshop can open must still export;
  blend keys outside [#2](https://github.com/kpgalligan/doctored/issues/2)'s qualified table are written but appearance-unqualified;
  descriptions, RAW layers and warps export as pixels; link groups, saved channels, guides, ruler
  origin, EXIF/XMP/IPTC, mask link state are not written (layer locks are, `psd_locks.rs`).
- Flattened mode writes one raster named "Composite".
- Out of scope: ZIP output, thumbnails, Doctored-private payloads.
- UI: File > Export as PSD… (`ImageDocument+PSDExport.swift`); MCP twins `inspect_psd_export` and
  `export_psd` (`AgentServer+PSDExport.swift`). FFI: `dr_doc_psd_export_report`, `dr_doc_save_psd`.
  PSD is not a `save_copy`/Batch/Action output; `export_psd` is not an Action step (`Action.notAStepTool`).
- **Phase-2 gates** (fixed before any GIMP run, never widened; `scripts/verify_phase10b_external.py`;
  reference direction is the reverse of 10A — GIMP's loads are the reference):
  1. Manifest (from the source document, never the importer) vs an independent decode
     (`scripts/psd_qualification_decode.py`) — exact structure, samples, masks, profile, PPI; preview RGB 0 codes.
  2. GIMP `file-psd-load` tree vs that decode — exact, after removing GIMP's synthesized clipping folders.
  3. GIMP reopened projection vs manifest render — ≤ 2 codes (514 at 16-bit); `styled_effects` recorded, not
     gated (GIMP reads no `lfx2`).
  4. GIMP `file-psd-load-merged` vs decoded composite, plus black/white backdrop comparison.
  5. Release app: `inspect_psd`, layered open, `inspect_psd_export`, `export_psd`; re-export byte-identical
     except the declared style-case preview.
- Corpus: `core/tests/fixtures/psd_10b/` (seven cases incl. `depth16`, `clipping`, `styled_effects`), built by
  `core/tests/psd_export_corpus.rs`. Round-trip/unit coverage: `core/tests/psd_export.rs`. Loader
  conventions recorded in [`scripts/psd-exchange-probe/README.md`](../scripts/psd-exchange-probe/README.md).

**v1 formats.** The layered export writes one of three containers, chosen beside
the mode (`DrLayeredContainer`; MCP `format: psd | psb | tiff`): a classic PSD, a Large Document PSB
(version 2: u64 section and channel lengths, u32 RLE row counts, 300000 px per side) or a TIFF whose
flat strip is the composite and whose tags 34377/37724 carry the same records. They share every
record, loss and observation; only the limits and the sentences that name the format differ, and a
PSD refused for a side or for size names PSB. `psd_framing.rs` is the one home of the widths.
In an 8- or 16-bit RGB document an adjustment layer never refuses: one with no Photoshop record is
baked into a canvas-sized pixel layer holding the picture up to and including it (`psd_export_bake.rs`),
one loss per layer. The
report's `bake_scratch_bytes` is three canvas-sized RGBA planes at the document's depth; the
multiplier is measured (24 MP, both depths, `core/examples/psd_bake_admission.rs`), and 2 missed.

## Rejected alternatives
- Refusing export past the import envelope — would block files Photoshop can open.
- Deriving expectations from Doctored's importer — gates use the source document and an independent decoder.

## Consequences
- Any new document feature must either map to a PSD construct, be refused with a reason, or appear in the
  export report — never dropped silently.
- The 8-bit writer's exact bytes are pinned by goldens in `core/tests/fixtures/psd_10c/` (see
  [phase-10c-depth-status.md](phase-10c-depth-status.md)).

## Open items (verified 2026-09-26)
- `DONE` — 16-bit export (then refused): `dr_precision_doc_save_psd`, `a3d29b7`.
- `DONE` — Adjustment layers (then refused): fourteen kinds written as Photoshop records (`psd_adjustment.rs`); unmapped kinds bake into a pixel layer (`psd_export_bake.rs`), and a CMYK or 32-bit document refuses an unrecorded adjustment by name.
- `DONE` — Layer styles (then refused/baked): written as `lfx2`/`iOpa`/blending ranges (`fc70079`); bake only where Photoshop cannot hold it.
- `DONE` — Group clipping dropped by Doctored's importer: retained since the 10C constructs slice (plan §10C).
- `DONE` — Photoshop's own reading of exported effects: Run 3 (plan §10C, `d1b6c27`).
- `DROPPED` — Human confirmation that the menu item greys out for 16-bit documents: moot, 16-bit export is now enabled (`ImageDocument.swift` `validateUserInterfaceItem`, "Both depths").
- `DONE` — PSB (v1 formats, above); CMYK, Grayscale and 32-bit float output from those documents ([cmyk-mode-status.md](cmyk-mode-status.md), [grayscale-mode.md](grayscale-mode.md), `psd_export_float.rs`).
- `DROPPED` — ZIP output; masks wholly outside their layer (GIMP drops them): outside the claim (plan §10 boundary).
- `DONE` — Photoshop appearance of blend keys and clipping ([#2](https://github.com/kpgalligan/doctored/issues/2), three takes, `psd_appearance_578_take.py` in the private archive; RGB, 8 and 16 bits, max 2 / mean 0.5 codes per tile): QUALIFIED Normal, Darken, Multiply, Color Burn, Linear Burn, Lighten, Screen, Color Dodge, Linear Dodge, Overlay, Soft Light, Hard Light, Pin Light, Exclusion, Subtract, Hue, Saturation, Color, Luminosity, Pass Through; Difference, Lighter Color, Hard Mix at 100 % Fill only; OUT Dissolve (its own dither), Darker Color (99 codes on a luminosity tie), Vivid Light (4), Linear Light (2, mean 0.52), Divide (7). The table is `psd_export_preflight::QUALIFIED_BLEND` / `QUALIFIED_AT_FULL_FILL`; CMYK keeps the four. Shipped with it: the clip rule measured on those takes (CL2, [10D](phase-10d-clipping-status.md)), `iOpa` always on Color Burn / Color Dodge / Difference pixel records, Fill F1/Fb for five special modes. Outside, and reported per file: a clip base below 100 % Fill (128 codes) and a clipped group, which Photoshop reads unclipped from the folder record, the divider or both and will not create (take3).
