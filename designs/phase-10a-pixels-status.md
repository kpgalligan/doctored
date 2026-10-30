# Complete admitted PSD raster rectangles (ADR)

- **Status:** Accepted — closed (superseded as the 10A status record by [phase-10a-qualification.md](phase-10a-qualification.md))
- **Phase / dates:** 10A raster slice, 2026-09 (before the 2026-09-13 10A closeout)
- **Original:** [archive/phase-10a-pixels-status.md](../archive/phase-10a-pixels-status.md) — full history, measurements, receipts
- **Key commits:** `2a7d042`

## Context
The `psd` 0.3.5 crate renders layers into a canvas: it clips off-canvas samples, floods absent alpha over
the whole canvas, narrows heights to u16, drops RLE row tables and mis-handles zero bottom/right edges.
That destroys exact signed layer storage. The slice had to keep the complete stored rectangle, signed
offset and exact straight RGBA8 of admitted RGB/grayscale layers without forking the dependency or adding a
second decompressor.

## Decisions
- Preserve each admitted raster's **full signed rectangle and exact straight RGBA8**, including wholly
  off-canvas pixels and RGB under zero alpha. Missing transparency = alpha 255 inside the rectangle.
  RGB needs channels 0/1/2; grayscale needs channel 0 (copied to RGB). Missing/partial color refuses.
- `psd_group_records` retains borrowed channel slices and rectangles and stays the sole framing
  implementation.
- The dependency sees a **metadata-only PSD view**: original blend/name/extra bytes in original ordinal
  positions, empty rectangles, zero channels, tiny RGB canvas. It still interprets names, properties and
  tree; its parent/ordinal sequence must agree with the walk. Canvas size and ICC/PPI come from the original
  `psd_container::Container`; pixels come from our own borrowed descriptors.
- `core/src/psd_layer_pixels.rs` copies Raw samples directly; each PackBits channel goes through the
  existing composite codec seam (`decode_composite`) and its red scalar output is placed into the right
  RGBA component. Mixed Raw/RLE per layer is supported. `validate_packbits` is the single framing-only
  validator. No premultiplication, no canvas clipping.
- Admission (before any wrapper, codec copy or plane allocation): nonempty positive rectangle, ≤ 30,000
  px/side, ≤ 100 MP; aggregate raw pixels (incl. group placeholders) within native 400 MP. **Empty raster
  records refuse** the layered choice (no native empty-rectangle representation; no 1×1 substitute).
  Channel IDs recognized and unique; Raw lengths exact; RLE rows decode to exactly the width with no
  unclaimed data. ZIP/unknown compression on non-mask channels refuses, even on discarded folder/divider
  records.
- **Peak forecast** `psd_layer_pixels::peak_forecast`: `4I + 8M + 4P + 4L + 2E + 4C + 64MiB` bytes
  (I source length, M metadata spans, P retained pixels, L largest plane, E largest encoded channel,
  C canvas pixels), saturating, refused past `psd_layer_pixels::import_envelope_bytes()` = `process_working_limit()`
  (2 GiB at the test pin). It bounds layered decode
  work after the file has been read — not the read, not other documents.
- A layer failure never silently opens the saved composite; the composite remains explicitly selectable
  with its own limits.
- Masks in this slice were bounded slices disclosed and dropped without decoding (superseded by the mask
  slice).

## Rejected alternatives
- Using the crate's channel parser/renderer — clips, floods alpha and mis-frames masks.
- Forking the crate or writing a general replacement parser — unnecessary with the metadata-only view.

## Consequences
- The forecast formula is shared: later slices extend it (masks add `A`; 16-bit adds `sample_bytes`) and the
  PSD export preflight evaluates the same function to warn about reopen size.
- Tests: `core/tests/psd_layer_pixels.rs`; app verifier `scripts/verify_phase10a_pixels.py`.

## Open items (verified 2026-09-26)
- `DONE` — Mask import: [phase-10a-masks-status.md](phase-10a-masks-status.md).
- `DONE` — Native-depth (16-bit) PSD: [phase-10c-depth-status.md](phase-10c-depth-status.md).
- `DONE` — Independent Photoshop/GIMP reopen: [phase-10a-qualification.md](phase-10a-qualification.md); plan §10C Runs 2–4.
- `DONE` — ZIP for 16-bit layer/mask planes (`core/src/psd_zip.rs`, `5e0ed4d`).
- `TRACKED` — 8-bit ZIP channels, ZIP composites → plan §10C "what stays open".
- `DROPPED` — PSB: plan §10 boundary ("Full Smart Object interchange and PSB are separate commitments").
- `TRACKED` — Phase 9I quality gate (mentioned as open) → plan §9I.
