# Precision conversion, quantization and authoritative readout (ADR)

- **Status:** Accepted — open items remain ([#10](https://github.com/kpgalligan/doctored/issues/10), [#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 7E core contract, 2026-09-10 (regional-fallback notes 13A-5c; structure-preserving addendum 12H-1/12H-2)
- **Original:** [archive/phase-7-conversion-readout.md](../archive/phase-7-conversion-readout.md) — full history, measurements, receipts
- **Key commits:** `3bd4349`

## Context
The public 16-bit mode ([public mode](phase-7-public-mode-status.md)) needed a deliberate way to move a document
between depths, a single quantizer for 8-bit output and previews, a 16-bit export that does not re-render, and
sample/histogram readouts that report original u16 codes rather than byte display values. The core never
reinterprets a byte handle as typed.

## Decisions
- **Flattened conversion** (`core/src/precision_conversion.rs`) always yields **one raster layer**, keeping ICC
  profile, print resolution and EXIF/XMP/IPTC; editable structure, live adjustments and masks are discarded; saved
  channels carry (×257 / nearest) (guides/ruler origin cross since 12E-1c). The original immutable document stays in history; converting
  back never reconstructs layers.
  - **Byte→u16:** render with established byte evaluation, then ×257 exactly — preserves byte appearance, adds no detail.
  - **u16→byte:** render the authoritative u16 projection, then nearest or ordered-dither quantization; export can do
    this on its captured source without replacing the master.
- **One quantizer** (`core/src/precision_quantize.rs`, `quantize_with_context`) for export and byte previews: nearest is
  `round(code/257)`; ordered is a fixed **canvas-anchored 8×8 Bayer** with threshold `(rank + 0.5)/64`, computed as
  `floor(code/257 + threshold)` in exact integers. RGB follows the chosen policy; **alpha is always nearest, never
  dithered**; exact promoted codes survive every threshold; straight RGB is quantized even when alpha rounds to zero.
  Dithering happens in the captured encoded RGB space — not a profile conversion, no perceptual claim.
- **Admission before work:** both directions expose retained/output/scratch plans; one-raster typed native capacity is
  checked before allocating; source, other host owners, intermediates and result all count against the working limit.
  Promotion additionally reserves **64 × aggregate raw layer-description bytes** (the legacy interpreter parses whole
  JSON values), so an oversized description can refuse before rendering with the original intact.
- **Cancellation:** legacy raster stacks (masks, pointwise adjustments, opaque metadata) use the cancellable tile
  compositor (13A-5c-2a); groups/styles use the byte regional tree sampled in ≤ 1024×64 tiles (13A-5c-2b); **only a
  guided Shadows/Highlights keeps the declared full-frame fallback**, polling per row. Typed rendering and both
  conversion loops poll cooperatively. Failure returns no document and releases the reservation.
- **Captured projection export** (`core/src/precision_projection_export.rs`) writes an already-retained u16 projection
  through the PNG16/TIFF16 codec and atomic metadata writer — no second render. The host must capture document,
  projection and revision together; the core checks RGBA16 UNORM, exact dimensions and exact ICC bytes (it cannot
  prove which revision produced the pixels). Codec reservation `48 × pixels + 16 × width + 128 MiB`; shared packet
  allocations are counted once. Encoders poll an optional job control as their bytes move (`cancel_io`); refusal
  and cancel leave the destination untouched.
- **Readouts** (`core/src/precision_stats.rs`): samples return exact center RGBA, exact component sums, count and
  unrounded means; reach 0/1/2 = 1/3/5 square, out-of-bounds dropped, hidden RGB included. Histograms have 256
  display bins (RGB, Rec. 709 encoded luma, alpha) plus **exact endpoint counts on original codes 0 and 65535**,
  independent of buckets. RGB/luma count nonzero-alpha pixels; alpha counts every visited position. Stride > 1 reports
  sampled positions only — never present as full-frame totals. 1 MiB reservation, cooperative cancellation; statistics
  never become a paint/save source.
- **Capability decisions** (`core/src/precision_capability.rs`): unknown operation/target/depth fails closed; admission
  of a family never authorizes a byte fallback. Detailed in [operation support](phase-7-operation-support.md).
- **Structure-preserving alternative (12H):** beside the unchanged flattened path,
  `core/src/precision_conversion_structure.rs` (8→16, codes ×257, plan names exact/different/discarded/blocked records)
  and `core/src/precision_conversion_narrow.rs` (16→8, float→16, float→8 per record with this same quantizer and
  `tone_mapped_u16`; masks/channels nearest). Conversion into float (8/16 → 32, every record carried) is
  `core/src/precision_conversion_float.rs`: each u16 code becomes the profile's TRC decode.

## Rejected alternatives
- Dithering alpha — alpha is coverage; noise there changes edges and compositing.
- Re-rendering the projection for 16-bit export — duplicate full-frame allocation over the public retained budget.
- Presenting outer histogram buckets as clipping counts — a 65,536-code ramp puts 129 codes in each end bucket.

## Consequences
- Every 8-bit output/preview must use `quantize_with_context`; the Bayer anchoring and undithered alpha are numerical
  contracts ([carryover "Contracts that remain intentional"](phase-7-carryovers.md)).
- Tests: `core/tests/precision_conversion.rs` (every u16 code vs integer/f64 oracle, recursive Bayer, block mean error
  ≤ 1/128 code), `core/tests/precision_stats.rs`, `core/tests/precision_projection_export.rs` (independent PNG16/TIFF16
  decode of every code, exact memory boundaries, atomic refusals).

## Open items (verified 2026-09-26)
- `DONE` — selection-weighted / named-backdrop statistics (this API has none): carryover P7-07, verified 9F
  (`scoped_analysis`).
- `DONE` — public-app and native 24 MP workflow checks (7E, [public mode](phase-7-public-mode-status.md)).
- `DONE` — structure-preserving conversion in every direction: carryover P7-30, verified 12H-1/12H-2.
- `OPEN` [#10](https://github.com/kpgalligan/doctored/issues/10) — guided Shadows/Highlights full-frame fallback in
  promotion (the guided sampling plates, 13B-9; carryover P7-31 closed 2026-11-12).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — the PNG16 encoder is polled only while its
  finished output is copied out (carryover P7-32); the export workspace and `save_projection16` both poll the
  PNG16/TIFF16 encoders under their control.
