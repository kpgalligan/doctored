# High-depth geometry (ADR)

- **Status:** Accepted — open items remain ([#2](https://github.com/kpgalligan/doctored/issues/2), [#13](https://github.com/kpgalligan/doctored/issues/13), [#31](https://github.com/kpgalligan/doctored/issues/31))
- **Phase / dates:** 12F (carryover P7-28), runs 12F-1 … 12F-5, 2026-09-20 → 2026-09-22
- **Original:** [archive/phase-12f-geometry-status.md](../archive/phase-12f-geometry-status.md) — full history, measurements, receipts
- **Key commits:** `c83614f` (12F-1), `dd1c7fc` (12F-1b), `3e56019` (12F-1b2), `8e42353` (12F-1b3), `7f9d55a` (12F-1b4), `f285a29` (12F-1c), `07d9153` (12F-2), `6400648` (12F-3 … 12F-5), `3ae0c80`

## Context
Before 12F, 16-bit documents had numeric resize, zero-angle crop, quarter turns, flips and per-layer affine/perspective
(`core/src/precision_geometry.rs`, `PrecisionTransformSheetController`), but no typed Canvas Size, straighten, arbitrary
rotation, interactive Move/Crop/Free Transform, set/group transforms, retained-source geometry, or any canvas geometry at
32-bit float. 12F filled that out in ten runs, keeping the byte ops as the specification and every edit pure and
one-undo-step. The constant pressure was: exact ops must stay bit-exact (pointer-equal where nothing moves), filtered ops
must match an f64 reference, and float must never resample extended-range samples without an oracle.

## Decisions
- **Typed ops mirror the byte contract, never re-specify it.** `GeometryEdit::CanvasSize` is a generalized Crop:
  rasters/masks are `WorkKind::Keep` with a moved offset (same allocation), channels padded with 0, chrome via
  `doc_guide::padded_guides` / `padded_origin` (`core/src/precision_geometry_canvas.rs`, whose module doc is the contract).
  Rides `dr_precision_doc_edit`; no format change.
- **Exact no-ops are successful no-ops at all three depths.** `canvas_size` to the same size, `rotate` by an identity
  angle, a full-canvas zero-angle `crop`, and a `flip`/180° whose result equals its input answer
  `{"ok": true, "changed": false, "note": …}` via `structureNoOp` — no history step, no dirtying, no recorded step.
  `DrDocument::canvas_resize` and the five byte exact methods return `None` for it. *Why:* a replayed Action or batch
  over already-matching documents must not halt on `on_error: stop`.
- **Float: no geometry gate.** The exact ops (Canvas Size, zero-angle Crop, quarter turns, flips) stay Nearest
  permutations that copy extended-range samples bit for bit;
  the filtered ones (Image Size, straighten, arbitrary rotation, affine/perspective and their sets, `mask_transform`)
  resample premultiplied extended-range f32 through the shared walks (`sampling::{accumulate_extended,
  straight_extended}`), dropping an output alpha below 2^-24 and clamping RGB to the layer's own visible range (D3).
  `float_gate`, `REFUSED_GEOMETRY` and the host's straighten/rotate-canvas refusals are deleted. Warp takes the same
  finish, applied at once with no persisted field.
- **Typed straighten = one op.** `GeometryEdit::Straighten { matrix, filter, x, y, width, height }` (serde `"straighten"`,
  all keys required) equals byte `straighten_layers(m)` + `transform_channels(m)` + `crop`. Every raster and every mask
  is `plan::affine(source, T(−x,−y)∘m, …)`; channels resample straight from the old canvas; PPI never
  transposed; lock-exempt. The window is signed and may lie past any canvas edge (`crop_background::window_fits`
  at both depths, chrome `padded_*` by (−x, −y)), with an optional Background fill (`doc_crop_extend`).
- **One angle parser per op.** `DocumentPrecision.cropAngle` (0 absent, `.nan` unreadable) and
  `DocumentPrecision.rotateDegrees` (90 absent, `.nan` unreadable) are read by every depth gate and the handler, so an
  unreadable angle refuses by name at every depth instead of silently becoming 0/90.
- **Angle 0 never enters `RasterDocument.straightenedCrop`** (`app/Sources/CropTool.swift`). Both call sites keep their own
  zero-angle `cropped` route. *Why:* a plain crop needs no resample, and the typed path bypasses `cropped`'s
  artwork-description capture/restore. (The byte
  `straighten` answers an inner-window identity with the plain crop, and a stack with nothing to write as unchanged
  layers, so an angle that moves no pixel still crops; over the whole canvas it is MCP `crop`'s `changed:false`.)
- **Arbitrary canvas rotation** (`core/src/doc_rotate.rs`, `core/src/ffi_rotate.rs`): positive = clockwise;
  `dr_canvas_rotation_plan` (no handle; classification codes `DR_ROTATION_*`) is the only place the rule lives — the
  sheet, MCP and `ActionGeometry.canvasAfter` all ask it. The box is `ceil(bw) × ceil(bh)`
  (measured against Photoshop) with the old canvas's top-left at the integer `(⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)`. Exact quarter turns
  (via `doc_transform::exact_form`, `EXACT_EPSILON` 1e-9) **must** delegate to `rotate90/180/270` in both core and
  host — on a `W+H`-odd canvas the box rule would place the picture half a pixel off (63×48: `tx, ty` 47.5, −0.5). Uncovered
  corners are transparent, channels 0, guides padded, selection carried, default filter bicubic. 16-bit twin:
  `GeometryEdit::RotateCanvas { degrees, filter }` sharing `doc_rotate::{rotation_kind, rotated_canvas}`. The
  byte op is reached only through `dr_doc_geometry_edit`'s `rotate_canvas` request, run as a cancellable background job.
- **Described layers lose their description when resampled.** Straighten and filtered rotation drop text/shape meta in the
  same edit and report `rasterized_layers` + `note`; the sheet asks first.
- **Selection is carried** through every affine canvas-geometry change at every depth
  (reversing "selection is cleared"): crop, Trim, Crop to
  Selection, Canvas Size, Reveal All, the rotations, the flips, Image Size and the straightening crop map it with
  the canvas, in the same undo step (`app/Sources/SelectionCanvasMap.swift`). Perspective Crop still clears.
- **Retained sources.** Canvas Size was lifted first (12F-1) on `effective_source`'s lazy `offset − rendered_offset` fold.
  12F-2 then kept the original master under every whole-document op (resize, crop, turns, flips, straighten, filtered
  rotate) and group/linked-set affine and perspective at both depths; `SourceTransform::Affine` / `Projective` with
  conditional native words 51/52 and an `RSPJ` table inside identity wrappers 45/46. `RetainedImagePolicy` no longer
  has a whole-document refusal set.
- **Set transforms.** Byte (`core/src/doc_geometry_set.rs`) and typed `GeometryEdit::AffineSet` / `PerspectiveSet`
  (`core/src/precision_geometry_set.rs`), sharing `geometry_set::expand` fixed-point membership: atomic,
  `doc_align::transform_layers` rules (linked masks ride, unlinked stay), groups transform descendants, an adjustment its
  mask only. Multi-layer Free Transform records as `transform_layers`. Float admits the filtered sets too (see Float,
  above).
- **Interactive geometry at 16 bits (12F-1c).** Move, Free Transform handles (affine and ⌘-corner perspective) and the
  Crop box are admitted; preview and commit share one editor capture and builder; a preview is display-only,
  the commit is the numeric typed op; a gesture whose document moved under it refuses by name. Typed `get_document` rows
  carry `offset_x` / `offset_y` beside `offset`.
- **Warp (12F-3)**: a bounded per-layer control lattice retained with pre-warp pixels (re-editable), one conditional native
  word nested inside 45/46; identity/integer fields are exact copies. At float a set is applied at once and keeps no field (no WARP
  binding, no format word), so float sets compound and Undo adjusts or removes one. **Source-coordinate geometry (12F-4)**:
  8G `RZLM` local planes remap under decoder geometry migrations, plus lens distortion and transverse CA as source-stage
  geometry behind a recipe version bump. **Byte paint recordability (12F-5)**: all six byte paint/retouch tools capture
  canonical dabs; one `SoftBrush.rasterizeDabs` traversal serves commit and replay.
- **PSD clip-run predicate (12F-1 carried fix).** A clip base has a visible clipped member iff a visible sibling exists in
  the contiguous clipped run directly above it at its own level (groups read via their folder record) —
  `psd_export::clip_base` (`core/src/psd_export.rs`). "Renders" means an enabled
  Gradient/Colour Overlay, Satin or Inner Glow that its settings do not skip (`style_knockout::has_grouped_interior_effect`,
  mirroring the renderers' `skipped_by_settings`). Report sentences and the 8-bit regional dry run.
- **Budget.** A float raster destination is charged twice in `precision_geometry_plan::build` (RGBA32 is 16 B/px).

## Rejected alternatives
- Carrying the selection through Canvas Size — declined for byte parity. Reversed (**Selection is carried**, above).
- A live preview in the Rotate Arbitrary sheet — would need an RGBA8 transform plate; the sheet shows the planned canvas
  size from the core instead.
- Committing an RGBA8 preview plate from an interactive gesture — a preview is never a source.
- Refusing Canvas Size on retained sources (the old MCP behaviour) — the fold already made it correct; lifted.

## Consequences
- Any new canvas op must decide its exact form via `exact_form` and return `None` for a no-op rather than an
  identical copy.
- 90° and 89.999° are deliberately different operations (permuted guides + transposed PPI vs. shifted/dropped guides).
- After a recorded arbitrary rotation, later `relative`/`anchor` Action geometry is remapped against the expanded canvas
  and does not follow rotated content on a different-size target — a stated limit.
- `precision_geometry_plan.rs` is past the ~600-line soft cap; new logic belongs in `precision_geometry_canvas.rs`
  (`mod canvas`) or a new submodule.

## Open items (verified 2026-09-26)
- `DONE` — Filtered geometry at 32-bit float (Image Size, straighten, arbitrary rotation, free transform, filtered set
  transforms, mask transforms) is admitted, checked against the f64 oracle in `../core/tests/float_filtered_geometry.rs`.
  Warp (`DONE`, `../core/tests/float_warp.rs`) and
  resampling during float export (`DONE`,
  `../core/tests/float_interchange.rs`) followed on the same finish.
- `DONE` — The C2 residual: `has_grouped_interior_effect` no
  longer counts a grouped-interior effect its settings skip (opacity 0, a size-0 edge glow), via the one shared
  `skipped_by_settings` the renderers use, so it neither discloses nor over-charges the 8-bit regional dry run
  (2026-09-27).
- `OPEN` [#2](https://github.com/kpgalligan/doctored/issues/2) — Measure Photoshop's `blend_interior_as_group` with
  `fill_opacity < 1` on a clip base with a visible member: C2's premise (the flag moves only the four grouped
  contributions) is unqualified there.
- `DONE` — Stale module doc in `../core/src/doc_rotate.rs` (R10 and the `rotate_canvas` refusal list) now says
  retained sources ride since 12F-2 (2026-09-26).
- `DONE` at RGB 8/16 — P7-38 (the Background mark) and PSD `cmls` (layer comps' names, comments and
  visibility); `OPEN` [#31](https://github.com/kpgalligan/doctored/issues/31) — both in the colour modes;
  `OPEN` [#13](https://github.com/kpgalligan/doctored/issues/13) — a comp's position and appearance.
- `DONE` — P7-39 (`blend_interior_as_group` on clip bases), 2026-10-13: see
  [phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md).
- `DONE` — Lens/camera database and calibration for 12F-4: `../core/src/lens_db.rs`, `../core/src/lens_model.rs`,
  `../core/src/sampling_lens.rs`.
- `DONE` — Typed twin of arbitrary rotation (`GeometryEdit::RotateCanvas`, `7f9d55a`); content-dependent flip/rotate
  no-ops (`7f9d55a`); interactive drag at depth (`f285a29`); set/group/retained geometry (`07d9153`; no `whole` set in
  `../app/Sources/RetainedImagePolicy.swift`); warp, source geometry, byte paint recordability (`6400648`).
- `DONE` — `get_document` offset spelled two ways (fixed in 12F-1c).
- `DONE` — Cancellation-test "never observed live" hole in `phase8e_coverage` / `precision_layer_blending` /
  `refinement_cutout`: their default cancel tests now park the worker mid-work and cancel it there
  (`common::probe::cancelled_parked_at`, build-velocity Phase 10), so every run observes the cancel live.
