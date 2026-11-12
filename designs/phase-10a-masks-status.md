# Ordinary independent PSD raster masks (ADR)

- **Status:** Accepted — closed (superseded as the 10A status record by [phase-10a-qualification.md](phase-10a-qualification.md))
- **Phase / dates:** 10A mask slice, integrated 2026-09-13
- **Original:** [archive/phase-10a-masks-status.md](../archive/phase-10a-masks-status.md) — full history, measurements, receipts
- **Key commits:** `b802115`

## Context
With complete raster storage and the outside-coverage model in place
([phase-10a-pixels-status.md](phase-10a-pixels-status.md),
[phase-10a-mask-foundation-status.md](phase-10a-mask-foundation-status.md)), ordinary 8-bit PSD layer and
folder masks could be retained as editable Doctored masks. PSD has many mask forms (vector, real-user,
parameterized, relative); only one narrow, well-specified form was admitted, with everything else disclosed
per file and malformed admitted data refused.

## Decisions
- **Admitted form:** one `-2` channel with a 20-byte mask body, absolute signed bounds, outside default
  0 or 255, flags containing only the optional disabled bit; Raw or PackBits over the mask's own nonempty
  rectangle (independent of layer rectangle and canvas). All raw coverage bytes are retained, including
  off-canvas. Density 1 and feather 0 are the only imported parameters. The body's two trailing bytes are
  padding.
- Imported masks are **explicitly unlinked** (`linked=false`); the report says PSD linkage was not inferred
  or preserved. *Why:* the spec distinguishes position-relativity from a content link.
- Folder masks follow verified folder IDs (duplicate names, empty groups). A group's native raw offset is
  its mask anchor. Masks on folders dissolved by the ten-level limit are reported losses; a mask on a
  discarded bounding divider **refuses** the layered choice.
- `psd_group_records` stays the only record parser and lends metadata/channel slices to
  `core/src/psd_raster_masks.rs` (`MaskRecord::classify`); no allocation during classification.
- **Unsupported forms are per-file losses, never decoded:** missing descriptor/channel pairs,
  non-20-byte/extended bodies, non-endpoint defaults, rendered/obsolete-invert/unknown flags, mask
  parameters, `-3` channels, vector masks, `vmsk`/`vsms`/`lmgm`/`vmgm`, unknown mask compression (an 8-bit
  ZIP mask, words 2 and 3, has opened since `22ea5a9f`; `core/tests/psd_zip8.rs`).
- **Malformed admitted data refuses layers** (bad rectangles, Raw lengths, row tables, PackBits packets,
  trailing bytes) — never an implicit dropped mask or a composite fallback. All retained channels are
  validated before any plane allocation.
- Decoding reuses `psd_layer_pixels::validate_channel`, `validate_packbits` and `psd_framing`'s one
  PackBits decoder (`unpack_packbits`); no second decompressor.
- Admission: widened signed arithmetic, 30,000-px side, 100 MP plane, shared native aggregate mask
  limit — checked before channel validation, including disabled and folder masks. Layered forecast becomes
  `4I + 8M + 4P + A + 4L + 2E + 4C + 64MiB` (A = aggregate mask samples; L/E include masks) under the
  saturating envelope `psd_layer_pixels::import_envelope_bytes()` = `process_working_limit()` (2 GiB at the test pin). Imported documents also pass native save
  preflight. This bounds decode work after the file read, not the read itself.
- Native persistence uses the foundation's byte28 wrapper only when an outside-255 mask exists, else v10.
- The compatibility report distinguishes retained observations, actual mask losses and general limits.

## Rejected alternatives
- Decoding unsupported mask payloads "best effort" — would claim appearance not qualified.
- Inferring PSD mask linkage — the format's relative bit is not a content link.

## Consequences
- New mask forms need their own contract and independent evidence; they must not acquire meaning by
  loosening `MaskRecord::classify`.
- Tests: `core/tests/psd_raster_masks.rs` (53-case corpus via `DR_PSD_MASK_FIXTURES`); app verifier
  `scripts/verify_phase10a_masks.py`.

## Open items (verified 2026-09-26)
- `DONE` — The position-relative flag (bit 0): read as Photoshop reads it since 2026-10-11 — the stored rectangle is canvas coordinates on rasters and folders, the flag recorded and not applied; the earlier bit-0 normalization (`1992d43`) is superseded. [phase-10a-relative-masks-status.md](phase-10a-relative-masks-status.md).
- `DONE` — Native-depth PSD masks: [phase-10c-depth-status.md](phase-10c-depth-status.md).
- `DONE` — Independent-application exchange: GIMP corpus in [phase-10a-qualification.md](phase-10a-qualification.md); Photoshop mask agreement Run 3 (`d1b6c27`).
- `DROPPED` — Extended/vector/parameter masks, density/feather import, PSD linkage: explicit exclusions of the closed 10A contract ([phase-10a-qualification.md](phase-10a-qualification.md)). The app has vector masks of its own, and an 8- or 16-bit RGB PSD's `vmsk` with no raster mask beside it is read and written ([v1-paths.md](v1-paths.md)); `vsms` is not read, and every other export writes the baked mask.
