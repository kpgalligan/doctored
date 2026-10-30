# Native paint and fill (ADR)

- **Status:** Accepted — closed (verifies P7-13, P7-14)
- **Phase / dates:** 9H, September 2026
- **Original:** [archive/phase-9h-paint-fill-status.md](../archive/phase-9h-paint-fill-status.md) — full history, measurements, receipts
- **Key commits:** `93a45cf` (Complete native paint, pressure replay and fill parity for phase 9H)

## Context
16-bit painting had to generate coverage beyond 256 levels, honour pressure size/opacity and airbrush
timing, and replay exactly from Actions — plus bring every byte paint/fill/gradient/dodge-burn tool to
native depth with independent selection gating — without changing any legacy byte kernel or Action.

## Decisions
- **Record what the gesture actually deposited.** New native strokes record pressure-dependent diameter
  and opacity and every 20 Hz airbrush Timer deposit; smoothing keeps the existing spline/leash; the pixel
  brush keeps snapped one-pixel squares; mouse pressure is 1. Pressure Opacity is an additive option.
- **RZBR version 1 Action payload** (`app/Sources/NativeBrushStroke.swift`): `version: 1`,
  `encoding: "le_f64_xy_diameter_opacity"`, base64 `dabs`, `hardness`, `roundness`, `angle_degrees`,
  `pixel`. Each dab = four LE binary64: canvas x, y, diameter, deposit opacity (flow × pressure opacity;
  stroke opacity applies once, separately). Positive angles are counterclockwise on screen. Unknown
  versions/fields, bad types, non-finite geometry, incomplete payloads and excess counts refuse before
  publication.
- **MCP generation:** `stroke_version: 1` with `points`, optional `pressures`, elapsed `times`,
  `pressure_size`, `pressure_opacity`, `airbrush`, `duration`; resolved to canonical dabs before dispatch and
  recording, so saved commands carry dabs. Times start at 0 and never decrease; airbrush ticks every 0.05 s
  at the interpolated endpoint.
- **Legacy meaning.** At 8 bits unversioned point calls/Actions keep the platform byte footprint; at 16 bits
  (since 13A-6c-2) they generate native dabs through the same walk. Byte painting, eraser, fill, gradient,
  dodge/burn and retouch kernels are unchanged. Native coverage is an additive path with its own bounded
  FFI dab array (`core/src/ffi_brush.rs`); the 64 KiB edit-request limit is unchanged.
- **Coverage geometry.** Soft ellipses evaluate the shared smoothstep in binary64 at pixel centres; hard
  ellipses use an explicit version-1 8×8 area approximation (`core/src/brush_coverage.rs`). Deposits
  accumulate continuously and round once to u16. Authored RGB is continuous in sRGB or document space.
  The canvas overlay is display-only, never the commit source.
- **Paint semantics.** All 27 paint blend modes via the shared kernel; Eraser is destination-out, mode-free,
  alpha-only, preserving hidden RGB; Dodge/Burn keep six range/direction combos preserving alpha; gradients
  keep independent continuous RGBA endpoints; masks/channels/RGBA planes get scalar gradients with separate
  endpoint alpha.
- **Storage growth.** Brush, selection fill, color-match bucket and gradient grow raw layer storage only where
  new ink changes a previously absent pixel; erasing absent storage is a no-op with no Undo entry.
- **Bucket fill.** `fill_mode: "color_match"` (anchored seven-projection tolerance, at native precision, 0–255 units,
  fractional allowed; contiguous/noncontiguous) or `"selection"`. New colour-match requires positive
  selection coverage at the seed (code 1 eligible); historical typed `contiguous: false` without `fill_mode`
  keeps its selection-wide meaning; 9G scalar bucket calls keep their seed admission.
- **Separate gates.** New paint/erase/dodge/retouch requests pass stroke coverage and native selection as
  separate planes, multiplied before storage rounding (tiny selection survives). Clone uses the product;
  Healing/Spot Heal keep the half-coverage stroke contour for the synthesis domain and apply native
  selection at writeback. Legacy retouch keeps combined coverage.
- **Quick Mask** records canonical dabs (no full-canvas RLE Action); 6- and 8-byte RLE Actions still replay.
  Native brush/selection work captures document/revision/target/view/selection, is admitted and
  cancellable, and refuses stale results.
- **Bounds:** 50,000 dabs (`MAX_BRUSH_DABS`), 10,000 authored points, diameters 1–5,000 and 2 billion work
  units (`MAX_BRUSH_WORK`; 1–512 and 256 M before; the healing brushes stay 3–200, their solve
  costs by area, and the five retouch brushes 1–200); over-limit MCP requests fail atomically, never a prefix, while a canvas stroke that
  reaches a cap stops laying ink and commits what was laid. No new document/history format
  version — versioned tool arguments live in Actions.

## Consequences
- RZBR v1 and the legacy 8-bit unversioned meaning are frozen replay contracts.
- 12A later added byte canonical dabs on the same rasterizer; 13A-6c-1 changed the work charge to the stroke
  footprint (hard-edge band only is area-integrated).
- Larger strokes/canvases beyond these caps are 13D's job, not a cap raise.

## Bounds and qualification
Dedicated FFI suites check every paint blend, soft coverage, pressure accumulation, partial/tiny alpha,
separate selections, raw offsets/expansion, bucket matching, gradients, dodge/burn, cancellation, work limits
and NULL/error atomicity. Production MCP (`scripts/verify_phase9h.py`), actual-source UI with synthetic tablet
events (`scripts/verify_phase9h_ui.py`) and 60 immutable legacy outputs (`scripts/verify_phase9h_compat.py`)
pass; native brush error ≤ 1 code vs an independent footprint reference. Display P3 differences 0–1 codes
against the built-in sRGB authored-profile reference.

## Open items (verified 2026-09-26)
- `TRACKED` — Larger or tiled strokes, larger history/canvas targets, broader resource qualification →
  carryover P7-34 and P7-33 (13D); plan §13A/§13D.
- `DONE` — Remaining 12A replay work (multi-layer gestures, byte canonical dabs, profile-qualified colour) —
  `7211b97`.
- `TRACKED` — P7-12/9I refinement/colour-decontamination gate → carryover P7-12.
