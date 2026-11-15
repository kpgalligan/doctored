# The Background Eraser (ADR)

- **Status:** Accepted — shipped
- **Date:** 2026-11-15
- **Issue:** part of [#18](https://github.com/kpgalligan/doctored/issues/18) (brush dynamics remains
  there)

## Context
Lifting a subject off a background whose colour varies across the frame is a brush job: the Magic
Eraser clears one region of one colour per click, and the Eraser clears whatever it touches. A brush
that samples the colour under its centre as it moves and erases only what is like it follows the
subject's edge without a selection. Doctored already decides "like" (the Magic Wand's predicate, in
every colour mode), already walks four-connected regions (`selection_region::flood`), already lays a
brush tip's coverage dab by dab (`brush_coverage`) and already erases under a coverage (the paint
walk's Clear, the typed `erase_native`). What it did not have is a coverage that depends on both the
dabs and the pixels under them.

## Decisions
- **The tool.** The Background Eraser sits in the Eraser's rail slot after the Magic Eraser; bare E
  cycles the three erasers. Its options bar: the tip (Size 1–1000 px, default 40, Hardness, Spacing,
  Angle, Roundness, Smoothing, Pressure size), Sampling (Continuous, Once, Background Swatch; default
  Continuous), Limits (Contiguous, Discontiguous; default Contiguous), Tolerance (0–255, default 32)
  and Protect Foreground (off). Opacity and flow are pinned to 100 and there is no airbrush, blend
  mode or Sample All Layers: a reached pixel is erased by the tip's coverage alone, and the tool reads
  what it erases. Tool options only (UserDefaults): no document state, no DRDC word.
- **Reference colour.** A dab's hot spot is the pixel holding its centre. Continuous: each dab's own
  hot-spot pixel. Once: the hot-spot pixel of the stroke's first dab whose hot spot is on the canvas
  and not fully transparent, for the whole stroke. Background Swatch: the background colour (over MCP
  `sample_color`, or `sample_ink` on CMYK), as the document's own codes. A Continuous dab whose hot
  spot is off the canvas or fully transparent erases nothing; so does a Once stroke before such a dab.
- **Like colour.** The Magic Wand's predicate on colour alone: R, G, B, R−G, G−B and R−B each within
  Tolerance of the reference, both read with alpha at full, so a half-erased pixel stays like its
  neighbours and a second pass keeps erasing it; a fully transparent pixel is never reached. A CMYK
  document compares the ink's naive RGB (`selection_region::ink_as_rgb`), a Lab document the Lab
  wand's key, a 32-bit document its display codes (a value above white compares as white), every
  other document its stored codes: exactly what the Magic Wand compares there. R, G and B compare as
  bytes at every depth (a u16 code read as its nearest byte, `selection_similar`'s **Resolution**),
  and Tolerance is in 8-bit codes.
- **Limits.** Discontiguous reaches every like pixel under the tip. Contiguous reaches the like
  pixels four-connected to the hot spot through like pixels the tip covers, within the dab's box; a
  dab whose hot spot is not itself reached (unlike the reference, protected, or uncovered by its own
  tip) erases nothing. No third, edge-preserving limit: Doctored has no rule of its own for one.
- **Protect Foreground.** A pixel like the foreground colour (same predicate and Tolerance; over MCP
  `protect_color` / `protect_ink`) is never erased and stops the contiguous walk.
- **Coverage and the erase.** Per pixel and dab, `c = tip coverage × dab opacity` where reached, else
  0; dabs accumulate `old += c·(1 − old)` in order and the field is quantized once, exactly as the
  brush's (with every pixel reached it is `dr_brush_u16`'s, sample for sample). Every dab samples the
  layer as it was before the stroke, so the result does not depend on how the stroke was split into
  ticks. The erase is the Eraser's: `alpha' = alpha × (1 − coverage × selection)`, colour kept.
- **One kernel, the existing erase.** `core/src/background_eraser.rs` computes the field (one module,
  no document type). At 8 bits `dr_doc_background_eraser_layer` erases it through the paint walk's
  Clear over the stroke's footprint frame; on a typed document
  `dr_precision_doc_background_eraser_coverage` answers the canvas-sized plane and the host hands it
  to `erase_native` with the selection as its second gate, as the Magic Eraser hands it the wand's
  region. One stroke plans at most 256 million work units (the footprint, and per dab its tip work
  and its box), Smudge's bound for Smudge's reason: the 8-bit canvas reruns the whole stroke from
  the gesture's base on every tick, so the stroke erases live; a typed stroke commits as one job.
  The canvas session stops laying ink at the bound (one beep); MCP refuses by name.
- **Targets.** A layer's pixels only: a mask, colour or ink plane, alpha channel or the Quick Mask
  refuses by name, and so do an adjustment layer and a group. A text, shape or fill layer is
  rasterized first (the prompt in the UI, silently over MCP). A pixels lock refuses with the lock's
  sentence and a transparency lock (the layer's own or an enclosing group's) by name.
- **Modes.** RGB at 8, 16 and 32 bits, Grayscale, Duotone, Lab, CMYK (8 and 16 bits) and Indexed
  (the erased pixels take the transparent index, else the palette entry nearest the background
  colour, white over MCP). Bitmap refuses for good: it has no transparency to erase into.
- **MCP and Actions.** `background_eraser_stroke` mirrors the stroke (`points` or `native_stroke`,
  the tip keys, `tolerance`, `limits`, `sampling`, the colours); `size` defaults to the tool's 40
  px with or without `stroke_version`; `blend_mode`, `flow`, `opacity`, `airbrush`,
  `pressure_opacity` and `aliased` refuse by name. A canvas stroke records it with the resolved
  colours, so a replay reads no swatch; a swatch's stored CMYK ink is read and recorded
  (`sample_ink`, `protect_ink`) only on a CMYK document, and a stored Lab ink is read only on a
  Lab one, where it records as its sRGB hex, so a replay of a Lab stroke whose swatch lies outside
  sRGB compares the clipped colour.
- **Edit ▸ Fade** follows a stroke: `background_eraser_stroke` is in `FadeStep.strokeTools`, weighed
  by the pixels the stroke changed (`designs/v1-shortcuts-fade.md`).
- **Checks.** `core/tests/background_eraser.rs` (an oracle written from this rule, its own walk and
  pixel-mode tip; the soft tip against `dr_brush_u16`), `scripts/verify_background_eraser.py` on
  owned Debug instances, the jobs-probe case `background eraser`, and the four mode drives' tool
  lists. The canvas feel (the hover ring's centre cross, a long drag with a large brush) is a hand
  check.
