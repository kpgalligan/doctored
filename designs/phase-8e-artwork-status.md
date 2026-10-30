# Phase 8E — retained artwork at native depth (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 8E, 2026-09-11
- **Original:** [archive/phase-8e-artwork-status.md](../archive/phase-8e-artwork-status.md) — full history, measurements, receipts
- **Key commits:** `23cf25d` (follows 8D `943dbf5`)

## Context
Text and shape layers were editable only in byte documents; in typed documents they were a Phase 7 raster
subset, and generated coverage was quantized to 256 levels with colors stored as rounded hex. 8E makes text and
shapes retained, re-renderable artwork at both depths with continuous authored color and native area coverage,
while keeping the saved raster authoritative so documents open without fonts. It verified P7-04 and P7-05.
Shared retained-source rules are in [phase-8-retained-source-status.md](phase-8-retained-source-status.md).

## Decisions
- **The raster is authoritative; the description re-renders it.** Native/recovery reopen shows the saved raster
  with no font lookup. Editing, numeric affine, whole-image resize/rotate/flip and Assign/Convert Profile
  regenerate from retained parameters; a byte preview never becomes the source of a native rerender.
- **Description v3:** continuous normalized sRGB RGBA plus ≤64 ordered, non-overlapping attributed color runs
  indexed by whole composed-character UTF-16 ranges. v1/v2 stay readable; older schemas never silently acquire
  v3. New authored colors outside normalized sRGB refuse (no rounded-hex fallback).
- **Description v4:** linear-light ink at 32 bits (`"ink":"linear_srgb"`, written only in a float document);
  depth changes convert on read (`ArtworkInk`).
- **Assign Profile regenerates artwork** from authored sRGB into the newly assigned profile (unlike ordinary
  rasters, which are reinterpreted). Rendering uses the qualified matrix/TRC ICC path and f32 compositor with
  one final u16 rounding; byte documents quantize that once.
- **TextKit owns layout/shaping; Rust owns coverage.** The host captures each resolved glyph font, position and
  text matrix (emoji/CJK/RTL fallback fonts included) and decoration paths via the superclass callback through
  a bounded PDF path capture. Rust (`artwork_coverage.rs`, `artwork_render.rs`) flattens curves to a 1/4096-px
  bound and integrates exact linear-edge area (nonzero and even-odd). *Why:* geometric coverage beyond 256
  levels; this is explicitly **not** pixel identity with TextKit's hinted rasterizer.
- **Intrinsic glyph assets keep their disclosed precision:** color glyphs render in an RGBA8 sRGB context,
  monochrome bitmap glyphs as A8, with authored opacity/ink continuous and separate. They do not restrict
  generated outlines to 256 levels. Verified platform facts: outline-only `EBDT`/`bdat` strikes report empty
  bounds and are skipped; OT-SVG and `sbix` glyphs take the color path; the reachable outline-less non-color
  glyphs (Apple Chancery U+0338) draw no ink and are dropped rather than adding an empty A8 part
  (`outline_less_no_ink` in the artwork host probe). The inked A8 path stays a fail-safe, qualified only by
  core composition.
- **Fonts are not embedded.** Saved pixels stay usable; a rerender needing an unavailable base font refuses.
  Outlines follow the installed font implementation — no cross-OS outline identity promise.
- **Rasterize Artwork** (`rasterize_artwork`) drops only the description, keeping exact pixels/profile/mask;
  Undo restores editability. Typed artwork supports affine; true perspective requires explicit rasterization.
  Generation ignores the canvas selection; independent layer masks stay intact. Insertion lands at the slot
  `position`/`layer` names at both depths (since 12E-2b).
- **Persistence:** typed native **v20** adds a bounded `ADSC` table (`artwork_native.rs`) with an explicit
  source-table sub-version so mixed RAW/linked/ordinary assets coexist; byte descriptions use the existing
  opaque layer-metadata slot. Everything preflights before PNG decode. A raster cannot carry both an artwork
  description and a retained-image binding. Unknown future descriptions keep cached pixels and refuse semantic
  rerender; pre-v20 readers refuse the container.
- **Fail closed on metadata:** typed presence uses cached descriptor lengths, so an admission-denied full read
  can never classify artwork as plain raster; crop/affine restoration fails unless the expected description is
  present; a failed restore never publishes a raster-only fallback. Core pixel/geometry/Assign paths
  invalidate stale descriptions; the host supplies corrected description + pixels atomically.
- **Cancellation:** platform layout/capture is admitted, bounded and noncancellable on the main thread; value
  arrays then enter the single worker, where flattening/coverage/composition cooperate. A latched parent token
  hands cancellation to each fresh child token so it cannot be lost between stages; publication rechecks
  source identity/revision/target/selection (including modal color-sheet state).
- **Limits (refusals, not an RSS ceiling):** 4,096 UTF-16 units, 64 color runs, 256 components, 64 M combined
  component samples, 262,144 path points, 524,288 flattened edges (formerly 262,144), 256 M scan-work,
  64 KiB descriptions; coverage reserves 106 MiB plus output/row storage before flattening.

## Rejected alternatives
- Rounding authored color to hex / byte coverage for typed documents — defeats P7-05.
- Embedding font files — size/licensing; the saved raster already guarantees usability.
- Replicating TextKit metrics for decorations — capture the platform's own paths instead.

## Consequences
- Any new geometry/profile op touching an artwork layer must regenerate from the description or disclose
  description loss; never publish a raster-only result on failure.
- Text/canvas size growth must go through P7-34, not a cap bump.

## Qualification
Independent Rust coverage/composition references: exact fractional rectangles, 1,024 distinct coverage values,
winding/holes, quadratic/cubic integrals; straight-edge references within 1 u16 code, converged curve corpus
within 32 codes. Host: outline layout within a 2-px neighborhood and 2% alpha-area gate vs hinted TextKit
(actual drift 0.715%). External MCP in both depths: native ellipse/text alpha with 1,986/4,100 distinct values
survive reopen exactly; crash recovery, Actions, Assign/Convert and explicit rasterization pass.
Reproduction: `scripts/verify_phase8e.py`, [`scripts/artwork-text-probe`](../scripts/artwork-text-probe/README.md).

## Open items (verified 2026-09-26)
- `DONE` Part A — Outline-less non-color glyphs that draw no ink are dropped (Apple Chancery U+0338 no longer exceeds 256 parts; `outline_less_no_ink`); the inked A8 fallback stays a fail-safe qualified by core composition. Part B (a fixture font) won't do.
- `TRACKED` — Larger text (>4,096 UTF-16 units) and canvases → carryover P7-34 / plan §13D; regional processing → §13A–13C.
- `DONE` — Group/linked-set geometry and native text/shape regeneration; true perspective now asks for authorization and reports exact description loss (12F-2, `07d9153`). Warp/deform shipped in 12F-3 (`6400648`).
- `DONE` — Brush/selection coverage beyond bytes (P7-11/P7-13 verified by 9G/9H); P7-06 verified by 9F.
- `DONE` — Typed insertion at the named slot (12E-2b).
