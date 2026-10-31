# Typed (16-bit) geometry and sampling (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 7D, 2026-09-10 (request table extended by 12F-1 … 12F-2, 2026-09-20/21)
- **Original:** [archive/phase-7-geometry.md](../archive/phase-7-geometry.md) — full history, measurements, receipts
- **Key commits:** `7e60190` (7D); later `c83614f`, `3e56019`, `7f9d55a`, `07d9153` (12F)

## Context
Geometry on the separate RGBA16 document had to preserve every u16 code on exact paths, keep premultiplied alpha on
filtered paths, carry masks/channels correctly, and share kernels with the byte path without changing byte
arithmetic. Status and batch evidence: [geometry/paint status](phase-7-geometry-paint-status.md).

## Decisions
- **One strict request type**, `GeometryEdit` in `core/src/precision_geometry.rs`: JSON ≤ 64 KiB, `deny_unknown_fields`,
  unknown ops/filters refuse before any pixel allocation. Ops: `affine` / `perspective` (one raster leaf or one
  independent mask; matrix `[a,b,c,d,tx,ty]`, quad = source TL,TR,BR,BL destinations, canvas coords), `affine_set` /
  `perspective_set` (12F-2: a set, group or adjustment root expanded to its editable content), `resize`, `crop`,
  `canvas_size` (12F-1: generalized crop — every raster/mask keeps its allocation and moves its offset; channels are
  canvas-sized copies; guides via `doc_guide::padded_*`; lock-exempt; admitted at 16-bit and float; the one
  whole-document op a retained source survives), `straighten` (12F-1b2: one op equal to byte straighten + channel
  transform + crop; 16-bit only), `rotate_canvas` (12F-1b4: shares `doc_rotate::{rotation_kind, rotated_canvas}` with
  the byte op; identity is a no-op, exact quarters delegate bit-for-bit), `rotate90/180/270`, `flip_*`.
- **Filters:** `nearest`, `bilinear`, `bicubic` (Catmull–Rom), `lanczos3`. No aliases, no Gaussian substitution, no
  byte fallback.
- **Mask semantics:** a leaf-content transform carries a *linked* mask through the same canvas mapping in the mask's
  own extent; an unlinked mask stays put; mask-only edits keep raster allocation/placement; whole-document ops move all
  masks regardless of link, keeping density/feather/enabled. Feather radii keep pixel-unit meaning.
- **Resize policy** (same as byte): scale each stored extent and offset, round dimensions and offsets separately,
  minimum 1 px; resample whole planes including off-canvas data. **Crop** rebases offsets without trimming raw storage;
  channels get exact subrectangle copies. Overflowing destination coordinates refuse (never saturate).
- **Sampling ownership:** `core/src/sampling_walk.rs` owns coordinates, tap order and edges; `core/src/sampling.rs`
  owns kernel formulas and premultiply/unpremultiply. The byte path keeps its f32 arithmetic and image-crate resize.
- **Exactness:** flips, quarter turns, integer translations and nearest preserve raw u16 codes including hidden RGB;
  integer translations share the original allocation; typed nearest resize uses exact rational pixel-center selection.
- **Filtered sampling:** premultiplied f32 accumulation with no intermediate integer rounding; divide by the unrounded
  alpha before clamp/round; an output whose alpha rounds to zero has zero RGB; affine/perspective taps out of bounds
  are transparent; reduction widens support to suppress aliasing.
- **Typed transform weights in f64** (same polynomial/window formulas), normalized in f64, then into the f32
  accumulator. *Why:* Catmull–Rom cancellation near the support edge broke the error gate in f32 at low-coverage
  edges; this fixes it without duplicating kernels or widening the gate. Byte weights and typed resize weights unchanged.
- **Plan before allocate:** `core/src/precision_geometry_plan.rs` computes final canvas, retained/scratch bytes and
  work units, validating every destination and a hypothetical native reopen first. 100 MP plane, native aggregate,
  2-billion-work-unit limits and the working limit apply, including caller-retained state.
- **Purity/cancellation:** capture immutable handles, publish a complete new document or an error; semantic no-ops
  return no replacement (12F-1b4 made stored-state-identical rotate/flip successful no-ops at 8/16/float). Sample loops
  poll within rows and every 4096 samples/taps; the FFI owns the token from parse through publication; every exit
  clears its reservation.
- **Gates:** filtered storage error ≤ `2e-6 + 0.5/65535`; exact paths require exact codes (`core/tests/precision_geometry.rs`).

## Rejected alternatives
- f32 weights for typed transforms — exceeded the stored-output bound at low-coverage Catmull–Rom edges.
- Widening the error gate instead — gates are never weakened to pass.
- Saturating overflowing coordinates — silent data loss; refuse instead.

## Consequences
- New typed geometry must reuse `sampling_walk`/`sampling` and the plan-before-allocate shape; byte and typed canvas
  rotation must stay on the shared `doc_rotate` classification.
- Filtered geometry at 32-bit float (resize, affine/perspective, straighten, arbitrary rotation, warp) is admitted by
  the float gate (`FLOAT_FAMILIES` in `core/src/precision_capability.rs`): it resamples premultiplied extended-range
  f32 and clamps each layer's RGB to its own visible range (`float_document`'s D3), outside this contract's gates.

## Open items (verified 2026-09-26)
- `DONE` — group-content and adjustment-content transforms (refused in 7D): `GeometryEdit::AffineSet` /
  `PerspectiveSet`, families `geometry_affine_set` / `geometry_perspective_set` targeting raster/group/adjustment in
  `core/src/precision_capability.rs` (12F-2, `07d9153`).
- `DONE` — drag/interactive geometry at depth (12F-1c, `f285a29`); persisted warp (12F-3, `6400648`).
