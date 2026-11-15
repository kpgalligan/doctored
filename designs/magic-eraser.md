# The Magic Eraser (ADR)

- **Status:** Accepted — shipped
- **Date:** 2026-11-14
- **Issue:** part of [#18](https://github.com/kpgalligan/doctored/issues/18) (brush dynamics remains
  there; the Background Eraser shipped: [ADR](background-eraser.md))

## Context
A click tool that clears a region of like colour is the quickest way to drop a flat background from a
layer. Doctored already computes that region (the Magic Wand) and already erases under a coverage (the
Eraser, Edit ▸ Fill…'s Clear), at every depth and in every colour mode.

## Decisions
- **The tool.** The Magic Eraser sits in the Eraser's rail slot, right after the Eraser; bare E cycles the
  slot's erasers (the Background Eraser follows it). Its options bar: Tolerance (0–255, default 32), Contiguous (on), Sample All Layers (off), Opacity
  (1–100 %, default 100). The options are tool options (UserDefaults), so no document state and no DRDC
  word.
- **The region** is the Magic Wand's from the clicked pixel at Point Sample: every pixel whose R, G, B
  and A each lie within Tolerance of the seed's and whose halved cyclic differences lie within half of
  it (`selection_similar`'s module doc), four-connected to it when Contiguous. With
  Sample All Layers off it grows over the active layer's own pixels; on, over the composite. It is
  hard-edged (the wand's 0 or full coverage), so there is no anti-alias option. At 16 and 32 bits and in
  the colour modes the tolerance is in 8-bit codes (the typed wand reads its codes as bytes,
  `selection_similar`'s **Resolution**); at 32 bits the wand
  reads the display copy, as the Magic Wand does.
- **The erase** lands on the active layer only: `alpha' = alpha × (1 − opacity × region × selection)`,
  colour kept — the Eraser's rule with the region as its coverage. A selection gates it by its coverage;
  a click outside the selection or off the canvas does nothing, and a click that changes nothing leaves
  no undo step.
- **Targets.** A layer's pixels only: a mask, colour or ink plane, alpha channel or the Quick Mask
  refuses by name, and so do an adjustment layer and a group (in Quick Mask the canvas beeps, as it does
  for every tool but the brushes, the Gradient and the view tools). A text, shape or fill layer is rasterized
  first (the prompt in the UI, silently over MCP). A pixels lock refuses with the lock's sentence, and a
  transparency lock (the layer's own or an enclosing group's) refuses by name, since it keeps exactly
  what the tool changes.
- **Modes.** RGB at 8, 16 and 32 bits, Grayscale, Duotone, Lab and CMYK, as above. Indexed: the erased
  pixels take the transparent index, else the palette entry nearest the background colour (white over
  MCP), as the Eraser's do. Bitmap refuses for good: a Bitmap document has no Magic Wand region; the
  Paint Bucket with white does that job there.
- **No new pixel math.** 8-bit RGB is the Paint Bucket's region and fill with Edit ▸ Fill…'s Clear at
  the tool's opacity (`RasterDocument.bucketFilled(…style:opacity:)`); every typed document is the typed
  wand's region handed to `erase_native` as its coverage plane, the selection as its second gate
  (`MagicEraser.swift`). One undo step, recorded as `magic_eraser`, which is also the MCP mirror.
- **Edit ▸ Fade** follows a click as it follows the Paint Bucket: `magic_eraser` is in `FadeStep.strokeTools`,
  so the fade is weighed by the pixels the click changed (`designs/v1-shortcuts-fade.md`). Indexed offers
  Fade after no step. `scripts/verify_magic_eraser.py` M5 drives it.
