# Mask outside-coverage foundation (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10A mask prerequisite, integrated 2026-09-13
- **Original:** [archive/phase-10a-mask-foundation-status.md](../archive/phase-10a-mask-foundation-status.md) — full history, measurements, receipts
- **Key commits:** `a0715d9`

## Context
PSD raster masks store a rectangle plus a "default color" for everything outside it (0 or 255). Doctored
masks implicitly treated outside coverage as 0, so a reveal-outside PSD mask could not be represented.
This slice added that background to the byte and 16-bit mask models, everywhere a mask is evaluated, and
to native persistence, before any PSD mask import was attempted.

## Decisions
- `MaskProperties.outside_coverage` (`core/src/doc_mask.rs`) is raw coverage outside the stored mask
  rectangle. Admitted values **0 and 255 only** (others refuse); default 0. 16-bit masks promote to
  0/65535. It is independent of samples, size, signed placement, enabled, linked, density and feather.
- Every evaluation path honours it: byte and typed rendering, effective/raw mask reads, group gates,
  thumbnails, Apply Mask. Density stays `1 - density * (1 - raw)`.
- **Feather with outside 255** blurs the finite complement with the existing Gaussian/multiresolution
  kernel and complements the result. *Why:* the shared blur pads with zeros, which would darken the
  boundary; this avoids a second blur algorithm. Native planning counts the two extra complement passes.
- Derived-mask, aligned-mask, styled-shape and content-bounds cache keys include the background, even when
  cloned properties share raw buffers.
- Affine/perspective mask resampling uses the existing spatial walk with a constant background (nearest
  exact; filtered samples accumulate deviations from the background). Resize/crop/canvas/placement edits
  retain it.
- **Invert inverts the whole canvas** (reversing the first decision here, which inverted the stored samples only
  and left the rest of the canvas as it was). With no selection the stored samples invert and `outside_coverage`
  flips; with one, a raster entry's plane first grows to the union of its rectangle and the canvas, then inverts by
  coverage (`core/src/doc_mask_invert.rs`, `dr_doc_invert_mask`; the typed `scalar_edit` Invert follows the same
  rule). Every other scalar writeback still edits the stored samples and keeps the outside value. Replacing a whole
  mask via creation/replacement APIs yields a new default-0 mask.
- No new UI control or MCP mutation; `outside_coverage: 0|255` is reported additively in the byte
  properties JSON, typed inspection and `get_mask_properties`.
- **Native persistence** (`core/src/mask_outside_native.rs`): conditional wrapper **DRDC28 (byte) / DRDC29
  (typed)** inserts its u32 version after `DRDC`, before the exact prior version and body; after all prior
  tails it appends `MOUT`, u32 schema 1, u32 nonzero count, strictly ascending u32 bottom-first/postorder
  record indexes, each of which must own a mask (→ outside 255; unlisted → 0). Empty/duplicate/unordered/
  out-of-range/maskless indexes, bad schema, bad inner version and trailing bytes refuse in borrowed
  preflight before PNG decode or mask allocation. Size `16 + 4·count`.
- A document with no outside-255 mask uses the **exact prior writer and bytes**.
- The wrapper admits only an explicit list of inner versions; any later format version must be explicitly
  admitted rather than widening an old reader. In typed files `LBLD` precedes `MOUT` (the later four-mode
  slice adds blend30 as an inner body of mask29).

## Rejected alternatives
- Padding the blur workspace with the background value — would need a second blur algorithm.
- Redefining Invert on an infinite plane — would silently change existing edit semantics. Reversed by the whole-canvas
  layer-mask Invert: a mask inverted only inside its rectangle was the surprise.

## Consequences
- Any new mask evaluation path or cache must include `outside_coverage` in its semantics/key.
- New native versions that can contain masks must be explicitly admitted as inner versions of v28/v29.
- Tests: `core/tests/mask_outside_coverage.rs`, `mask_invert_whole.rs`, `precision_mask_invert.rs`; app
  verifiers `scripts/verify_phase10a_mask_foundation.py` and `scripts/verify_v1_masks_fills.py`; UI probe
  `scripts/selection-probe/mask_outside.swift`.

## Open items (verified 2026-09-26)
- `DONE` — PSD mask import on top of this model: [phase-10a-masks-status.md](phase-10a-masks-status.md) (`b802115`).
- `TRACKED` — Phase 9I prerequisite decision / carryover ledger (mentioned as unchanged) → plan §9I, P7-12.
