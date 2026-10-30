# Typed masks, channels, paint, artwork and healing (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 7D, 2026-09-10
- **Original:** [archive/phase-7-pixel-editing.md](../archive/phase-7-pixel-editing.md) — full history, measurements, receipts
- **Key commits:** `7e60190`

## Context
The typed `PrecisionDocument` needed persistent coverage and pixel-writing operations — masks, saved channels,
paint/erase/clone/fill, rasterized text/shapes and healing — while the public editor still used the byte document.
The overriding rule: no RGBA8 overlay, CGImage, clipboard bitmap or display preview may ever become a typed master.
[Geometry](phase-7-geometry.md) has its own sampling contract; batch evidence is in the
[geometry/paint status](phase-7-geometry-paint-status.md).

## Decisions
- **u16 scalar coverage.** Masks and saved channels store native u16. Masks have independent dimensions/placement,
  link flag, enabled state, density and feather; channels are canvas-sized named planes with session identities.
  Source alpha never passes through a byte coverage API.
- **Mask evaluation:** feathered coverage prepared as f32 with the shared Gaussian/multiresolution schedule; raw stays
  exact; full density has an exact identity branch; no retained typed blur cache (planned scratch, dropped per layer;
  the renderer reserves the largest mask workspace). Masks gate raster alpha, live adjustment strength, isolated group
  alpha or the pass-through contribution, staying f32 to the final store. *Effective*-mask inspection applies
  density/feather even when disabled (matches the byte inspector); rendering bypasses a disabled mask; raw reads
  ignore properties.
- **Effective-alpha floor 2^-96** (was 2^-32), admitting alpha 1 × mask 1 × partial opacity; positive underflow and
  positive products below the floor refuse (`core/src/precision_render.rs`). Error gates unchanged: `2e-6` float,
  `2e-6 + 0.5/65535` stored.
- **Native v13** adds raw little-endian u16 masks/properties per record and named channels at the document tail; v11/v12
  reads kept; framing and operator validation precede all decoding; writer, reader and hypothetical-edit plans count
  every serialized plane even when revisions share allocations.
- **Paint/fill** take a canvas-sized u16 coverage plane plus a continuous straight RGBA color with explicit
  `color_space: document | srgb` (sRGB through the 7C continuous ICC path). Normal paint uses the shared float
  source-over and rounds once on write; erase changes only alpha and keeps latent RGB; zero coverage/opacity or
  unchanged storage returns no edit.
- **Clone** reads a captured raster leaf at an integer offset, ignoring that layer's visibility/opacity/mask and always
  reading the original (also when source = destination); off-source/transparent source pixels do not paint; opaque
  full copies preserve all four codes.
- **Rasterized text/shapes** reuse the existing TextKit layout and shape path plans: AppKit draws alpha-only coverage
  on main (with admission before contexts), Rust combines one or two coverage/color planes in f32 into a new u16
  raster. The byte-coverage adapter multiplies by 257 and is **disclosed as 256 levels**; authored RGB stays continuous.
- **Healing:** explicit canvas-space solve rectangle, captured source at integer offset; coverage ≥ ½ enters the shared
  membrane domain; source alpha must be positive; destination alpha is exact; opacity (not fractional coverage) controls
  RGB writeback. Byte and u16 share one writeback, region classifier and Poisson multigrid (`core/src/poisson.rs`);
  the typed policy adds ≤ 32 refinement cycles and requires residual ≤ 1/16 u16 code, else the edit is discarded.
  A residual is not a solution-error bound — independent f64 harmonic references qualify tested domains within two
  codes. Admission: destination copy + coverage + 160 B per solve-window pixel + a sweep-work bound; no implicit
  reduced-resolution solve.
- **FFI surface** (strict UTF-8 JSON ≤ 64 KiB, `REQUEST_LIMIT` in `core/src/ffi_precision_edit.rs`):
  `dr_precision_doc_edit_plan` (admitted plan + exact input layout), `dr_precision_doc_edit` (pure edit, exact no-op
  returns NULL without error), `dr_precision_doc_read_plane` (raw u16 or once-rounded effective coverage, staged before
  publishing). Request families: `set_mask`, `remove_mask`, `mask_properties`, `set_channel`, `remove_channel`, the
  geometry ops, `paint`, `erase`, `clone`, `heal`, `rasterize`, `read_plane`; read targets `mask`, `effective_mask`,
  `channel`, `red`, `green`, `blue`, `alpha`. Unknown fields/targets refuse; no byte fallback. The FFI owns the job
  token from pre-parse admission (4 MiB) to publication; the final copy-out is noncancellable.
- **Swift:** calls stay in `app/Sources/DoctoredCore.swift`; scalar buffers carry dimensions, plane count, provenance and
  retained-payload leases; captures use the existing bounded worker/resource registry. No second Undo system.

## Rejected alternatives
- Feeding colored RGBA8 dabs or clone CGImages into a typed source — byte data would become the master.
- An implicit reduced-resolution heal solve — silently changes quality; refuse on admission instead.
- A retained typed blur cache — unbounded retention; prepare per evaluation within the plan.

## Consequences
- Every later typed paint/retouch/scalar feature (9D–9H) extends these request families and the "no byte master"
  rule; byte inputs that remain byte-valued (legacy brush footprints, glyph assets, Vision outputs) must be disclosed.

## Open items (verified 2026-09-26)
- `DONE` — mask/channel paint/filter/morphology targets, general RGBA-plane writeback, selection semantics (carryover
  P7-08, verified 9G).
- `DONE` — Sample All / current-and-below sources, fractional clone, source exclusion (P7-15, P7-16, verified 9D).
- `DONE` — paint extent expansion, other paint modes (P7-14, verified 9H); pressure and coverage > 256 levels (P7-13).
- `DONE` — editable text/shape descriptions, attributed colors, intrinsic color glyphs (P7-04, P7-05, verified 8E).
- `DONE` — content-aware synthesis/PatchMatch, spot healing, red-eye (P7-17, P7-18, verified 9D/9E).
- `DONE` — public UI/MCP/Actions/history/recovery integration (7E, [public mode](phase-7-public-mode-status.md)).
- `TRACKED` — text beyond 4,096 UTF-16 units (still enforced, `app/Sources/NativeArtworkEvaluation.swift`) and larger
  strokes/canvases → carryover P7-34 (13D).
