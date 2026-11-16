# Brush dynamics (ADR)

- **Status:** Accepted — slice 1 shipped (pen response curves on pressure and tilt, size and opacity
  jitter, scatter), and slice 2 (the tip shape: angle and roundness dynamics, pen rotation); colour
  dynamics, texture and dual brush proposed, open on
  [#18](https://github.com/kpgalligan/doctored/issues/18)
- **Date:** 2026-11-15; slice 2 2026-11-16
- **Issue:** part of [#18](https://github.com/kpgalligan/doctored/issues/18)

## Context
Before this, pen input reached a stroke only as pressure, straight onto size and opacity (two
checkboxes). A stroke is already a list of resolved dabs (`x, y, diameter, deposit`) plus one tip,
and everything downstream works on that list: the core rasterizers (`dr_brush_u16`,
`dr_brush_overlay_rgba8`), the CoreGraphics byte commit (`SoftBrush.rasterizeDabs`), every colour
mode, masks, channels, Quick Mask, recording (`native_stroke`), replay, Fade and undo.

## Decisions
- **A dynamic changes which dabs are laid, never how one is composited.** The core kernel
  `brush_dynamics` turns one nominal stamp (position, brush size, pen sample, direction of travel,
  stamp index, seed) into the dabs actually laid; the dab session lays those, and the canvas preview
  draws exactly those. So every depth and colour mode the three tools paint in, and every target
  they take (layer, mask, colour plane, channel, Quick Mask), takes slice 1 with no rule or refusal
  of its own, and no document state changes (no
  DRDC word). A stroke without dynamics never reaches the kernel and keeps its bytes.
- **The rule (slice 1).** Controls, curves and factors are in 0…1.
  - A response (`size`, `opacity`) has a control (off, pen pressure, pen tilt), a curve `C` (the
    natural cubic through 2–16 points from x = 0 to x = 1, `adjust_curves::Spline`; absent is the
    identity), a minimum and a jitter. Its control value `v` is the pressure, or
    `min(1, hypot(tilt_x, tilt_y))` (0 upright, 1 flat). Its factor is 1 for off or when the stamp
    has no sample for the control (a mouse; an MCP call without `pressures` or `tilts`), else
    `minimum + (1 − minimum) · C(v)`.
  - Nominal diameter `D0 = max(1, size · f_size)`; the walk's spacing follows it. Nominal deposit
    factor `O0 = f_opacity`; a dab's deposit is the tip's flow times its factor.
  - Stamp `i` lays `n` dabs: the scatter count (1–16), or with count jitter
    `max(1, round(count · (1 − count_jitter · u)))`, `u` from `SplitMix64::at(seed, i, 0, 1)`.
  - Dab `k` takes four draws `u1…u4` from `SplitMix64::at(seed, i, k, 2)` (top 53 bits):
    diameter `max(1, D0 · (1 − size_jitter · u1))`, factor `O0 · (1 − opacity_jitter · u2)`, offset
    `amount · D0 · (2·u3 − 1)` across the direction of travel and, with both axes,
    `amount · D0 · (2·u4 − 1)` along it; with no direction (a click, the first stamp, an airbrush
    tick at rest) the offset runs at the angle `2π·u4`. Amount 0–10 (0–1000 % in the sheet); 0 keeps
    the position bit for bit.
  - Jitter only shrinks a dab, so no dab is wider than the brush the cursor shows and the session's
    work charge stays an upper bound; scatter's extra dabs count against the 50,000-dab and work caps
    like any dab (the canvas stops ink, MCP refuses by name, with the existing sentences).
- **The rule (slice 2, the tip shape).** `angle: {control: off | rotation | tilt | direction,
  jitter}` and `roundness: {control: off | pressure | tilt, minimum, jitter}`, with the stroke's own
  tip angle and roundness passed to the kernel (`tip`). Dab `k` takes the fifth and sixth draws
  `u5`, `u6` of its stream (slice 1's four keep their order and values): angle
  `wrap(tip angle + a + 180 · jitter · (2·u5 − 1))` into (−180, 180], `a` the pen's barrel rotation,
  the tilt's direction `atan2(tilt_y, tilt_x)` (y up) or the heading's `atan2(−y, x)` (canvas y
  down), counter-clockwise on screen as `BrushTip.angleDegrees` is, and 0 for off or no sample;
  roundness `clamp(tip roundness · f · (1 − jitter · u6), 0.01, 1)`, `f` the control's
  `minimum + (1 − minimum) · v` with no curve (1 for off or no sample).
  - **The heading** is the last non-zero move between nominal stamps (`lastHeading`), so a stamp
    at rest or an airbrush tick at the pointer keeps the angle of the travel before it. It is zero
    until the stroke first moves: **the first stamp lays at the tip's own angle.** Scatter keeps
    reading the move from the stamp before (`dir`) and its no-direction rule.
  - **The MCP walk's ticks.** `generated` lays every spacing stamp, then its airbrush ticks, so a
    tick's move from the stamp before is a jump back along the path. A tick passes its own heading
    and leaves `lastHeading` alone: the direction of the `times` segment it lies on when that
    segment has length, else of the nearest earlier segment of `points` with length, else zero.
    A canvas tick lands at the pointer and needs none.
  - **Pen rotation and tilt on a rotated view.** The canvas reads `NSEvent.rotation` and
    `NSEvent.tilt` at every mouse-down and drag (`latchPen`) and turns both into the canvas frame
    by the view rotation (clockwise-positive on screen): the rotation adds it, the tilt vector
    turns counter-clockwise by it in its y-up frame. Rotation runs through the stroke spline and
    the walk beside tilt, on the shortest arc; the MCP walk interpolates `rotations` the same way.
  - **The shaped stroke.** A stroke with a tip shape records one `(angle, roundness)` pair per dab
    (`NativeBrushStroke.shapes`; the canonical stroke's `shapes`, base64 little-endian binary64
    pairs, written only when present). The core draws it through `dr_brush_u16_shaped`
    (`brush_coverage`, THE SHAPED STROKE), the byte commit and the canvas preview each dab at its
    own tip (`tip(forDab:)`). A stroke without a tip shape keeps its bytes at every stage.
- **Pressure and tilt in the walk.** Tilt (`NSEvent.tilt`) is read at every mouse-down and drag and
  carried through the stroke spline beside pressure (`StrokeVertex.tilt`), interpolated per stamp,
  so a dab's tilt is where the pen was at that dab. The MCP walk interpolates `tilts` the same way.
- **The seed and exact replay.** A canvas stroke draws a fresh seed at mouse-down; an MCP call passes
  `dynamics.seed` (default 0), so the same call paints the same pixels. The recorded step holds the
  resolved dabs (`native_stroke`), never the seed or the dynamics, so a replay is exact by
  construction and stays exact if a later release changes the kernel.
- **Actions geometry.** Because a step holds resolved dabs, an absolute replay is exact. A relative
  replay maps each dab's centre as a position (per axis) and its diameter by
  `ActionGeometry.lengthFactor` (`√(sx · sy)`), as for any `native_stroke`: scatter moves with the
  canvas per axis and is not drawn again. A dab's angle and roundness are not lengths, so its
  `shapes` replay as recorded at every mode. A Pencil step refuses remapping with the existing
  sentence (`nativeStrokeRefusal`).
- **Tools.** The Brush, the Pencil and the Eraser (`EditorTool.takesBrushDynamics`), on the canvas
  and over MCP (`brush_stroke`, `eraser_stroke`, `quick_mask_stroke`). The Pencil snaps and
  de-duplicates each resolved dab as it does a plain one. The size-1 pixel brush takes none (MCP
  refuses by name). Every other paint tool keeps `PaintToolOptions.dynamics` nil whatever a preset
  brings, shows no Dynamics… button, and refuses the MCP keys by name; Stroke Path strokes at one
  pressure with no dynamics, and `stroke_path` refuses the keys by name. The tip shape is the
  Brush's and the Eraser's (`EditorTool.takesTipShapeDynamics`; over MCP also `quick_mask_stroke`):
  the Pencil's tip is round, so its `dynamics.shape` is pinned nil, its sheet shows no Angle or
  Roundness rows, and `aliased` refuses `angle` and `roundness` by name.
- **Options.** `PaintToolOptions.dynamics` (nil whenever every value is its default). The two
  pressure checkboxes stay the pressure on/off: a response's stored control is nil (follow the
  checkbox) or Pen Tilt; choosing Off or Pen Pressure in the sheet sets the checkbox, and ticking or
  clearing a checkbox leaves Pen Tilt. The Dynamics… sheet (`BrushDynamicsSheetController`): Size and
  Opacity (Control, Minimum, Jitter, Curve… over the shared curve editor), Scatter (Amount, Both
  Axes, Count, Count Jitter), Angle (Control, Jitter) and Roundness (Control, Minimum, Jitter) for
  the Brush and the Eraser, Reset. Tool options only: no undo step, no Action, exempt from agent
  parity as every tool option is (a call passes `dynamics` on the stroke).
- **Presets.** A user preset may hold `dynamics` (file `schema_version` 2; a preset without them is
  still written at 1 and keeps its bytes; the `.doctoredbrushes` container is unchanged; an older
  build lists a version-2 file as one `unknown_version` row). Choosing a preset on a dynamics tool
  sets or clears the dynamics. The Brush's, Pencil's and Eraser's popups list every preset and match
  on the dynamics too; every other tool's popup lists only the presets without dynamics, so no two
  listed presets on any tool hold the same values and the `,` `.` keys reach every one. The Pencil's
  popup and keys list only the presets whose dynamics hold no tip shape, for the same reason. A
  save or an import that repeats another preset's six tip values and dynamics is refused by that
  preset's name.
- **MCP.** `dynamics` (`{seed, size, opacity, scatter, angle, roundness}`), `tilts` (one `[x, y]`
  per point, each −1…1) and `rotations` (one per point, −360…360 degrees, counter-clockwise) on the
  three tools; `dynamics` generates version-1 dabs whether or not `stroke_version` is given.
  Refused by name: any of the keys on any other tool, with `native_stroke`, `tilts` or `rotations`
  without `dynamics` or of the wrong length or range, a response's `control` together with
  `pressure_size` / `pressure_opacity`, a size of 1, `angle` or `roundness` with `aliased`, and a
  `native_stroke` holding `shapes` on any tool outside the three. `save_brush_preset` takes
  `dynamics` (no seed); `list_brush_presets` rows carry it.
- **Checks.** Core: `make t F=brush_dynamics`, against an oracle written from this rule (its own
  SplitMix64, natural cubic and angle wrap), and `make t F=brush_shaped_coverage` (shapes equal to
  the request's give `dr_brush_u16` bit for bit; mixed shapes against the test's own rotated-ellipse
  oracle). App: `scripts/verify_brush_dynamics.py` on owned Debug instances (seeded repeatability,
  the curve, scatter's band, replay of a recorded step under absolute and relative geometry, the
  tip shape against the plain stroke at the resolved angle and roundness, the heading at rest and
  on an L path's airbrush ticks, the refusals, presets) and the jobs-probe case `brush dynamics`
  (tilt and rotation through synthesized tablet events, the options and presets pinning, the walk's
  per-stamp tilt and rotation, a shaped canvas stroke, the Pencil's shape pinning).
- **Hand checks for the owner** (a real pen, the screen's focus): the Dynamics… sheet's layout with
  the Angle and Roundness rows, and its curve popover; a pen's pressure through a bent curve; tilt
  on size; heavy scatter and a shaped stroke looking the same live and at mouse-up; the Preset popup
  on Dodge not listing a preset with dynamics; pen rotation's sign (a barrel-rotation pen turned
  clockwise should turn a flat tip clockwise; `NSEvent.rotation` is read as counter-clockwise) and
  the tilt direction's (`NSEvent.tilt` read as y-up), with the view rotated and not; a
  direction-angled flat tip's first dab lying at the tip's own angle.

## Proposed: the open slices (for the batch that builds each)
- **Colour dynamics.** Per dab, the ink a seeded mix toward the background colour plus hue,
  saturation and brightness jitter. Needs a stroke that carries colour per pixel (premultiplied
  colour accumulated beside coverage, and a typed commit that takes it). Proposed limits: layer
  pixels only (a mask, plane, channel and the Quick Mask are coverage and refuse it by name); Normal
  mode only until the blend path takes per-pixel ink; the Eraser has none; Grayscale and Duotone take
  the mix and brightness only; CMYK, Indexed and Bitmap refuse by name.
- **Texture.** A library pattern tile (resolved into the step, as Pattern fill is), anchored at the
  canvas origin and scaled, multiplies each dab's coverage through Depth:
  `c' = c · (1 − depth · (1 − t))`. Coverage only, so every mode and target takes it; the Pencil
  refuses.
- **Dual brush.** A second tip walks the same path with its own spacing, scatter and stream; its
  stroke coverage multiplies the first's before the one quantization. The canonical stroke carries
  the second dab list. Coverage only, so every mode and target takes it.
