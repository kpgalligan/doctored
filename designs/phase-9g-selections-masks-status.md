# Native selections, masks and planes (ADR)

- **Status:** Accepted — closed (verifies P7-08–P7-11)
- **Phase / dates:** 9G, September 2026
- **Original:** [archive/phase-9g-selections-masks-status.md](../archive/phase-9g-selections-masks-status.md) — full history, measurements, receipts
- **Key commits:** `f457cdb` (Complete native selections, masks and channel editing for phase 9G)

## Context
16-bit documents needed selections, Quick Mask, masks, saved channels and individual RGBA planes that hold
and edit genuine u16 coverage end to end — including selection creation from native sources and camera
auxiliary mattes — while every existing byte ABI and operation keeps its meaning.

## Decisions
- **Authoritative u16 selection.** `CanvasSelection.Shape.mask16` owns canvas-sized u16 coverage; positive
  native coverage defines bounds even when it rounds to 0 on display. The grayscale image and contour are
  disposable byte display derivatives. `maskSamples16()` returns native samples (or promotes a legacy byte
  selection ×257); `maskBytes()` keeps its byte ABI meaning.
- **Algebra unchanged:** add = max, intersect = min, subtract = `min(a, max − b)`. Invert, feather, smooth,
  grow/shrink/border, affine transforms and Quick Mask use the bounded u16 selection API. Native coverage
  flows into selected tone/filters/Auto, retouch clipping, Patch, CAF, adjustment masks, channels and mask
  creation; byte display clipping never replaces the native write mask.
- **Quick Mask Action runs:** historical 6-byte encoding and new 8-byte encoding (LE u32 run length, then
  premultiplied u16 gray and u16 alpha); runs must cover the canvas exactly and gray ≤ alpha. Historical
  runs promote explicitly.
- **`get_selection`** keeps `coverage` at 0–255 and adds exact `coverage16`, the maximum and storage name.
- **Editing-state package version 3** (readers for 1 and 2): native selection files are exactly two LE bytes
  per canvas sample; current/previous selections keep their own type; v3 also stores channel-view state.
  Validation precedes materialization. (The package has since moved to v4 for linear-light evaluation —
  `app/Sources/EditingStateArchive.swift`.) Transient selections live only in the editing-state package;
  the native document stores reusable coverage as masks/channels.
- **One captured scalar request** for masks, saved channels and R/G/B/A planes: neighbourhoods read the
  original plane, canvas selection gates writeback, and only that plane changes (raw offsets, mask
  properties, other components and zero-coverage samples exact). Scalar colours use the core Rec. 709
  gray. Scalar Auto derives native levels (whole-target or weighted) and applies them without reanalysis.
  Apply Layer Mask uses effective coverage even when disabled and multiplies only native alpha.
- **Apply Image / Calculations / luminosity masks** use the shared blend table (HSL restrictions for scalar
  output) and publish one final document; compound reservation is the largest real plan plus live buffers;
  an undersized child token refuses before any change.
- **Inspection:** raw vs effective mask views are separate (effective applies density/feather/placement,
  ignores `enabled`); display reductions are never accepted as scalar editing input. MCP `histogram` with
  `source: "scalar"` names the target; `modify_scalar` mirrors Channels ▸ Modify Plane.
- **Source-derived selection** reads captured native sources directly (alpha, luminance, colours, raw planes
  via the core canvas read). Wand/Color Range/Quick Selection share the existing algorithms with native
  evaluation and u16 output; Wand keeps 0–255 tolerance units (fractional allowed); Color Range MCP adds
  `samples16`. Subject/object/people tools deliberately feed Vision a byte display derivative and keep
  declared u8 model coverage; they publish selection only and reject stale results.
- **Auxiliary mattes:** OneComponent8 promoted ×257, OneComponent16 kept, float quantized once;
  depth/disparity normalized over its finite range (near = white). Alignment uses the core bilinear scalar
  sampler. Each imported channel carries bounded provenance (source format, precision, orientation,
  dimensions, alignment policy); malformed present records error rather than silently dropping.
- **Native document v25** wraps a prior typed body and adds the validated channel-properties tail; channels
  with default properties and no provenance keep the older writer. Channel replacement keeps identity;
  duplication creates a fresh one.

## Rejected alternatives
- Presenting interpolated/aligned matte levels as extra source precision — provenance states decoded
  precision only.
- Running Vision on native pixels — the model's input and output are byte; that precision is declared.

## Consequences
- All byte ABIs (`maskBytes`, byte morphology, 6-byte Quick Mask runs, `coverage` 0–255) are frozen meanings.
- The opaque packed RGB10 HEIC auxiliary variant stays an explicit host-layout refusal.

## Qualification
Production MCP (`scripts/verify_phase9g.py`), actual-source UI (`scripts/verify_phase9g_ui.py`), legacy
editing-state/byte compatibility (`scripts/verify_phase9g_compat.py`) and ImageIO auxiliary probes
(`scripts/verify_phase9g_auxiliary.py`, all eight EXIF orientations, zero native-code error) pass; selected
filters stay within one code of independent references, and scalar Auto reproduces exactly. Normalized
histogram plot doubles have a 4-ULP JSON transport allowance; integer statistics are exact.

## The wand and bucket predicate (2026-10-13)
The Magic Wand and Paint Bucket (`selection_region::grow`; byte, 16-bit, float R1 codes) admit by Select ▸
Grow/Similar's seven-projection box anchored at the seed: R, G, B, A, R−G, G−B, R−B each within ±t
(`ReferenceRange::from_seed`). 16 bits compare u16 codes with t × 257; a gray mask/channel is |Δv| ≤ t.
- **Takes** (`scripts/photoshop-probe/blur_wand_take.py`; every click in a uniform 101×101 field, so Sample
  Size cannot matter). take1c (W1, t 0/1/10/32): P7 ≤ four-connected exact at t 0, 10, 32 for both tools;
  at t 1 Photoshop admits only grey ±1 shifts. take2 (W2, t 3/17/50/100): wand = bucket, P7 not exact —
  14,119 mismatched pixels over 16 cases a tool against the old per-channel box's 133,279. Every global
  mismatch has a difference projection at exactly t (Photoshop refuses some at t 1, 3, 17) or t + 1 (it
  admits some at t 50, 100); channels match exactly; t 50 contiguous is exact. Recorded wand and bucket
  Actions select and fill differently.

## Open items (verified 2026-09-26)
- `DONE` — Generated native brush/pressure engine for Quick Mask and scalar painting (handed to 9H) — `93a45cf`.
- `DROPPED` — Opaque packed RGB10 auxiliary layout "assigned to 11D": 11D kept packed 10-bit layouts outside
  the host packing seam by decision (carryover P7-23 row; `HostStillDecoder.layout` still refuses them).
- `DONE` — P7-12's other half: edge brushing and colour decontamination shipped 2026-10-11 in Select and Mask; the
  9I cutout gate is no longer a ship condition.
