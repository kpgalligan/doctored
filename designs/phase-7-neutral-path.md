# Preserving 16-bit neutral workflow (ADR)

- **Status:** Accepted — open items remain ([#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 7B, 2026-09-10
- **Original:** [archive/phase-7-neutral-path.md](../archive/phase-7-neutral-path.md) — full history, measurements, receipts
- **Key commits:** `2d9bc69`

## Context
7A chose u16 storage with f32 evaluation ([decision](phase-7-high-depth-decision.md)). 7B built the first
end-to-end typed path — import, a layer tree, rendering, native save/reopen and 16-bit export — as an internal
workflow with distinct Rust and Swift types, before any public mode existed (that came in 7E). The core
risk was a typed image leaking into a byte API or a byte fallback silently replacing a typed source.

## Decisions
- **Separate document type.** `PrecisionDocument` owns immutable u16 raster sources and a bounded tree of isolated
  Normal and pass-through groups, with policy `rgba16_unorm` storage + `encoded_rgb_f32_v1` evaluation
  (`EVALUATION` in `core/src/precision_document.rs`). Edits return new handles; unchanged opacity is no edit.
- **Separate FFI owners.** `DrPrecisionDocument` / `DrPrecisionImage` in `core/include/doctored_core.h` expose exact
  native-endian u16 reads and an explicitly named byte-preview conversion. `DrDocument`/`DrImage` contracts are
  unchanged; no typed handle can enter a byte editing API. Swift keeps source ICC apart from the byte display image.
- **Pass-through groups** evaluate children against the enclosing backdrop and gate the combined contribution with
  the premultiplied opacity kernel shared with the byte renderer (old arithmetic and exact 0/1 endpoints kept).
- **Preserving import only:** genuine unsigned RGB/RGBA16 PNG and classic TIFF; orientation applied as a sample
  permutation (PPI axes transposed); untagged RGB interpreted as sRGB **with an import warning**. Unsupported
  depth/profile/alpha/container semantics are errors — **no ImageIO byte fallback, no working-space adoption**.
- **Delivery:** PNG16/TIFF16 render the captured typed document through the shared metadata sidecar / TIFF layout,
  with per-packet carriage reporting and atomic publication. Neither native save nor export accepts an RGBA8 preview.
- **Native v11:** storage/evaluation declared before typed layer records; complete record structure and serialized
  capacity validated before any pixel decode; writer and reader share limits; lossless RGBA16 PNG layer payloads.
  Byte documents keep writing their legacy layout; the byte reader refuses typed versions before decoding, and a
  native failure never drops to the app's generic ImageIO fallback. Unknown feature/evaluation tags refuse.
- **Captures:** a Swift captured save retains one exact typed source while later immutable edits proceed; a
  versioned host archive stores that source plus a bounded manifest. The weak host registry inventories every live
  typed document/image in one deduplicated union (shared profiles across byte/typed families may be counted twice —
  conservative by design). Plans use real sample width, not 4 bytes/pixel.
- **Execution:** composite and preview loops poll the single-use job; PNG/TIFF dependency calls poll as their bytes
  move (`cancel_io`), and the compound export takes one control for its render and its encode (since 2026-11-12).
  Failures keep prior handles and destinations intact.

## Rejected alternatives
- Automatic working-space conversion or ImageIO fallback on import — would silently replace source samples.
- Reinterpreting byte handles as typed — breaks every existing byte caller's contract.

## Consequences
- Every later typed feature adds to `PrecisionDocument` and a versioned native wrapper; legacy byte evaluation stays
  separate. Public Undo/snapshot/recovery integration was deferred to 7E and delivered there.
- 7B's 24 MP workflow (9/9 stages at 6000×4000 under 2 GiB tracked, 2.75 GiB RSS) shared one source allocation
  across layers; it did not qualify independent multilayer sessions.

## Open items (verified 2026-09-26)
- `DONE` — masks, channels, adjustments, other blend modes, styles, geometry, paint/retouch and profile conversion,
  all unavailable in 7B (7C/7D, then carryovers P7-06…P7-20, verified).
- `DONE` — public mode, central Undo/snapshots/recovery (7E, [public mode](phase-7-public-mode-status.md)).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — uncompressed PNG/native raster payloads and
  duplicate serialized layers; codec calls that still cancel only between stages (carryover P7-32).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — independent multilayer/48 MP sessions (carryover
  P7-33).
