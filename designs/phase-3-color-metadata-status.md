# Phase 3 — TIFF, color and capture metadata (ADR)

- **Status:** Accepted — open items remain ([#15](https://github.com/kpgalligan/doctored/issues/15))
- **Phase / dates:** Phase 3 (3A–3D) of the [implementation plan](professional-photo-editor-implementation-plan.md), 2026-09-09
- **Original:** [archive/phase-3-color-metadata-status.md](../archive/phase-3-color-metadata-status.md) — full history, measurements, receipts
- **Key commits:** `f34201b`

## Context
TIFF dropped embedded ICC and real PPI, RAW/HEIC capture metadata was not carried, Exposure/White Balance
ignored the document profile, and PSD import hid its limits. Phase 3 fixed these per supported container,
versioned the colour evaluation so old documents keep their look, and surfaced PSD losses before editing.

## Decisions
- **3A Typed TIFF writer** (`core/src/tiff_encode.rs`, later shared by high-depth writers): embedded ICC in,
  document PPI out as inch rationals, orientation baked on import (quarter turns swap resolution axes),
  straight alpha declared. Classic TIFF, first image only.
- **3B Relocate metadata, never copy IFDs.** EXIF offsets are file-relative, so standard EXIF/GPS/Interop
  fields are relocated into a compact packet (`core/src/metadata_tiff.rs`); other private fields are
  reported omissions and TIFF export refuses a packet it cannot relocate whole. XMP/IPTC keep the 16 MiB
  packet caps (`MAX_DRDC_BLOB_LEN`). A MakerNote (tag 37500, BYTE or UNDEFINED) is relocated as one opaque
  value, so it survives a TIFF open and a TIFF export; offsets inside it are the maker's own and are not
  rewritten.
- **File Info.** Creator, copyright notice, description and keywords are authored per document
  (`core/src/file_info.rs`, `file_info_legacy.rs`, `doc_file_info.rs`): written to the XMP packet's
  `dc:creator`/`dc:rights`/`dc:description`/`dc:subject` (one description element; another program's are
  replaced), mirrored into EXIF IFD0 Artist/Copyright/ImageDescription when the document has an EXIF
  packet, and the four IIM datasets plus the IPTC digest dropped so they cannot contradict. Read order per
  field is XMP, then IIM, then EXIF. The EXIF write appends a new IFD0 at the packet end and reuses its own
  tail on a repeat edit, so no other byte moves; when the result would pass the JPEG APP1 bound the three
  tags are removed in place instead and XMP alone carries them. Undoable, `.doctored`-persisted (no version
  bump: the packets are existing state), carried by JPEG, PNG and TIFF exports. UI File ▸ File Info…
  (⌥⇧⌘I); MCP `get_file_info` / `set_file_info`, where an omitted key is unchanged and a recorded step
  carries only the keys it changed. Out of scope: GImage/GDepth XMP, Save As a flattened copy, writing IIM
  datasets, host-encoded HEIC/WebP exports, private EXIF tags other than MakerNote.
- **RAW/HEIC via ImageIO and a tiny TIFF carrier** (`core/src/doc_metadata_import.rs`): the carrier's
  pixels/profile/PPI are never adopted; a CFA DNG missing density gets upright PPI from the core scanner.
  Uninspected sources report `metadata_not_captured`. Guarantees are per container/packet, never blanket.
- **3C Versioned colour evaluation.** New RGB Exposure/White Balance store `params.evaluation = "document_v2"`
  and evaluate through the document's matrix/TRC profile (`core/src/adjust_calibrated.rs`; table in
  `core/src/adjust.rs`); absent or `"legacy_srgb"` keeps old numbers; scalar targets stay grayscale.
  Unsupported profiles refuse; a saved calibrated layer under one is an explicit render failure while native
  save still works. Writer v9 recorded this without a layout change. *Why:* saved documents must not change look.
  A matrix/TRC profile whose tone curves fail the precision check refuses a NEW or EDITED `document_v2` layer,
  destructive apply or 8-bit editable paste at both depths with the core's own-profile sentence
  (`adjust_calibrated::calibrated_refusal`, asked by the host through `calibratedRefusal`), but an EXISTING 8-bit
  layer in such a profile keeps rendering through the byte encoder, so its look survives reopen and merge down;
  a stored file of that state (`core/tests/fixtures/rz_byte_bad_gamma/calibrated_exposure.rz`) pins it through
  reopen, export and merge down. Adjustment Options… on such a layer offers Switch to Legacy sRGB Evaluation…,
  which reopens the sheet in `legacy_srgb` with the look change previewed and Apply as one undo step, the twin
  of MCP `edit_adjustment_layer` with `params.evaluation` `legacy_srgb`.
- **3D PSD compatibility notice.** Reports separate general limits, observations and verified per-file
  losses; the app offers layers vs stored composite before editing; MCP preflight returns a source
  fingerprint the open must echo (`expected_fingerprint`, `core/src/ffi_psd.rs`). Then: 8-bit RGB/gray only,
  composites Raw/PackBits, ZIP composites refused.

## Rejected alternatives
- Byte-copying EXIF IFDs — offsets break; relocate or refuse.
- Adopting carrier pixels/profile/PPI — the carrier is transport only.
- Changing Exposure/White Balance numerics in place — would alter every saved document.

## Consequences
- Calibrated operations carry an explicit evaluation version; unsupported profiles refuse.
- Each new container gets its own metadata slice and loss report.
- Tests: `core/tests/{metadata_tiff_tests,metadata_import,calibrated_adjustment_tests,psd_import_notice}.rs`,
  `scripts/color-metadata-probe/`, `scripts/verify_phase3.py` (independent libtiff/ImageMagick/ImageIO reads).

## Open items (verified 2026-09-26)
- `DONE` — BigTIFF opens at 8-bit, 16-bit and float with ICC, XMP, IPTC,
  EXIF and ppi read as for classic TIFF (`core/src/tiff_view.rs`'s `Tiff::file`); it saves back as classic TIFF.
- `OPEN` [#15](https://github.com/kpgalligan/doctored/issues/15) — Multipage TIFF: refused at 16 and 32 bits
  (`core/src/precision_codec.rs`, `core/src/tiff_float.rs`); the 8-bit open reads only the first page.
- `DONE` — High-depth TIFF (Phase 7, `core/src/precision_codec.rs`) and float TIFF (11F, `core/src/tiff_float.rs`).
- `DONE` — Detailed per-file PSD record reporting → 10A (`core/src/psd_feature_markers.rs`); 16-bit PSD → 10C
  (`core/src/psd_import16.rs`).
- `DROPPED` — ZIP stored composites / 8-bit ZIP channels stay refused (`core/src/psd_zip.rs`); the closed
  10A/10C contract is Raw/PackBits plus 16-bit ZIP planes (plan §10).
- `DONE` — File Info authoring, and a MakerNote kept through TIFF open and TIFF export (above). Tests
  `core/tests/metadata_file_info.rs`, `metadata_tiff_tests.rs`; drive `scripts/verify_phase11a_export.py` gate E8.
- `DROPPED` — Original packet bytes and private EXIF fields other than MakerNote: stated non-goal, never re-owned.
