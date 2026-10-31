# Genuine 16-bit RGB PSD import and export (ADR)

- **Status:** Accepted — open items remain ([#2](https://github.com/kpgalligan/doctored/issues/2))
- **Phase / dates:** 10C-1, 2026-09; Photoshop 16-bit import (Run 4) 2026-09-16
- **Original:** [archive/phase-10c-depth-status.md](../archive/phase-10c-depth-status.md) — full history, measurements, receipts
- **Key commits:** `a3d29b7` (16-bit import/export, depth-agnostic preflight), `5e0ed4d` (`Lr16` + ZIP, Photoshop fixture)
- **Photoshop harness:** runners under `scripts/photoshop-probe/` and the `verify_*photoshop*` drives live in the private archive `kpgalligan/doctored-dev`.

## Context
10A/10B were 8-bit only. 16-bit documents (`PrecisionDocument`) needed PSD exchange without any silent
depth conversion. Photoshop's own 16-bit files turned out to use a different layout (records in an `Lr16`
tail block, ZIP-with-prediction channels), and the `psd` crate is 8-bit only with a 30,000 header cap.
Per-construct preserve/rasterize/refuse rules are a separate slice
([phase-10c-constructs-status.md](phase-10c-constructs-status.md)).

## Decisions
- **Depth follows the document on both sides.** 16-bit RGB PSD opens as a 16-bit document; a 16-bit
  document writes 16-bit. No cross-depth conversion in either direction: a 16-bit file through the byte
  importer or flat image open refuses naming the 16-bit path; an 8-bit file through
  `dr_precision_doc_open_psd` refuses; cross-depth export requires the explicit conversion first.
- The 10A import and 10B export contracts apply unchanged at 16 bits (framing, Raw/PackBits, ordinary
  20-byte masks, folders, refusal order, "never silent").
- Carried at 16 bits: big-endian u16 planes (Raw or PackBits over `width*2` byte rows); typed rasters with
  straight RGBA16, hidden RGB and off-canvas retained; `PrecisionMask` u16 coverage, outside 0/255, enabled,
  unlinked; typed pass-through/isolated groups; opacity byte; flag bit 1; ICC 1039 parsed **once** into the
  single shared profile handle; PPI 1005. Stored composite un-matted with the widened-integer twin of the
  byte formula (alpha exact; color within `ceil(32767.5/a)`).
- Blend keys: `PSD_BLEND_KEYS` forwards, `psd_export_preflight::blend_key` backwards, filtered through
  `precision_layer_blend::COLOR_MODES`; unrepresentable keys import as Normal with a per-record report.
- **Typed groups keep their open/closed state**: retained on import (divider 1 open / 2 closed) and
  written back the same way on export.
- Export is written from the **host's authoritative projection** of that revision
  (`dr_precision_doc_save_psd`, like `save_projection16`); nothing is rendered inside the writer.
- `psd_container::Container::layer_info` is the **one** place deciding where layer records live: the
  ordinary layer-information section if non-empty, else (depth 16 only) the first `Lr16` tail block after
  the global-mask section (`8BIM` + key + u32 length, padded to a multiple of 4). `merged_alpha` reads from
  the same body. `8B64` or a bad tail signature refuses the **layered** choice only.
- `core/src/psd_zip.rs` is the **one inflate seam** (`miniz_oxide`) for 8-, 16- and 32-bit layer and mask
  planes and the composite: it validates the zlib header, inflates with the admitted plane size as limit,
  refuses any other length and undoes per-row big-endian prediction (word 3).
- `psd_framing::unpack_packbits` (re-exported by `psd_container`) is the one PackBits decoder at every
  depth, twin of `validate_packbits`.
- `psd_export_preflight` is depth-agnostic via `ExportInventory`: shared refusal order, sentences and
  worst-case arithmetic; only the first limitation sentence and `sample_bytes` differ.
  `psd_layer_pixels::peak_forecast` takes `sample_bytes`, so import admission and export preflight evaluate
  one forecast at either depth (inflated ZIP planes fit the existing term; no budget change).
- Import job control is begun **once** in `psd_import16::open16` and polled between planes;
  `PrecisionImage::from_rgba16` gets `None` (a single-use token cannot be begun twice).
- FFI: `dr_precision_doc_open_psd`, `dr_precision_doc_psd_export_report` (adds `"depth":16`),
  `dr_precision_doc_save_psd`.
- **Refused at 10C:** 16-bit grayscale, 32-bit float (`Lr32`), CMYK/Lab, PSB, 8-bit ZIP. All have since
  opened: PSB and 8-bit and composite ZIP (`22ea5a9f`), `Lr32` (`psd_import_float.rs`), Gray, CMYK and Lab
  ([grayscale-mode.md](grayscale-mode.md), [cmyk-mode-status.md](cmyk-mode-status.md),
  [lab-mode.md](lab-mode.md)). The 10B refusal set still holds at 16 bits: a classic PSD side > 30,000
  (the refusal names PSB), file > 2 GiB, mask density/feather.
- The 8-bit writer's exact bytes are pinned by goldens `core/tests/fixtures/psd_10c/{nested,flat}.psd`
  (`psd_export.rs::eight_bit_writer_bytes_match_the_committed_goldens`); regenerate only for an intentional
  8-bit writer change.
- Photoshop 16-bit import gates (Run 4, fixed before the run): D1 framing, D2 import, D3 base exact at
  0 codes; per-case tolerances in 8-bit codes — `empty` 0, `masked`/`group` 2, `shadowed` 4, `curves` and
  the carried stack 8; cases containing an omitted record (`brit`, `SoCo`) recorded, not gated.

## Rejected alternatives
- Mapping 16-bit rows onto a doubled-width 8-bit crate plane — fails past 15,000 px.
- Implicit depth conversion on import or export — violates "never silent".

## Consequences
- Tests: `core/tests/psd_depth16.rs` (independent builder/reader `tests/common/psd16_fixture.rs`),
  `core/tests/psd_zip16.rs`, `core/tests/psd_photoshop_depth16.rs` (Photoshop-authored
  `core/tests/fixtures/psd_photoshop/depth16/`); runner `scripts/verify_phase10_photoshop_depth16.py`
  (private archive).
  16-bit export reopen: `psd_10b/depth16` via [phase-10b-export-status.md](phase-10b-export-status.md).
- New inflate or PackBits decoding must reuse the single seams above.

## Open items (verified 2026-09-26)
- `DONE` — 10C-2 per-construct rules (styles, adjustments, Blend If, clipping, knockout/infx at 16 bits): [phase-10c-constructs-status.md](phase-10c-constructs-status.md); plan §10C; P7-21 verified.
- `DONE` — Typed/16-bit PSD clipping: 10D clipping slice (P7-20 verified).
- `DONE` — 16-bit ZIP/ZIP-prediction layer and mask planes (`psd_zip.rs`, `5e0ed4d`).
- `DONE` — Independent reopen of 16-bit exports (GIMP `psd_10b/depth16`) and third-party-authored 16-bit import (Photoshop Run 4).
- `DONE` — `SoCo` and `GdFl` carriage and `brit`: all carried at 8 and 16 bits (a 16-bit `GdFl` is written pixel-less, as Photoshop writes it).
- `DONE` — 8-bit ZIP channels and ZIP composites (`22ea5a9f`, `core/tests/psd_zip8.rs`); `Lr32` read and written (`psd_import_float.rs`, `psd_export_float.rs`).
- `OPEN` [#2](https://github.com/kpgalligan/doctored/issues/2) — measured adjustment-arithmetic gaps against Photoshop (e.g. `curves` 5 codes).
- `DONE` — Gray, CMYK and Lab PSDs ([grayscale-mode.md](grayscale-mode.md), [cmyk-mode-status.md](cmyk-mode-status.md), [lab-mode.md](lab-mode.md)); PSB (`22ea5a9f`).
