# Warp Text (ADR)

- **Status:** Accepted, not built — an item of [#19](https://github.com/kpgalligan/doctored/issues/19). The
  decisions below are what a build follows; the code does not exist yet.
- **Shares** the outline-map kernel (TP-7) with [type-on-a-path.md](type-on-a-path.md); its payload
  version is 8 ([text-ink-rect.md](text-ink-rect.md) TI-6).

## Decisions (WT-1..WT-7)

- **WT-1 — The description.** `TextLayerPayload` version 8 adds an optional `warp`: `{"style", "bend",
  "horizontal", "vertical"}`, each number in −1…1. All three at 0 is no warp and is never stored: the tools
  remove the key, and an edit that changes nothing registers no step. `warp` and `on_path` on one layer is
  "not a text layer"; a tool that sets one while the other is present refuses and names it.
- **WT-2 — The envelope** maps the text block's box (the left-aligned layout rect `TextLayer.layout`
  measures, `w × h`, centre `(xc, yc)`) onto itself bent. With `u, v` a point's fractions across and down the
  box, `c = 2u − 1`, `d = 2v − 1`, `p(t) = 1 − t²` and `b` the bend:
  - `arch`: `y′ = y − b·(h/2)·p(c)` (the whole line rises in the middle; glyphs stay upright)
  - `upper`: `y′ = y − b·(h/2)·p(c)·(1 − v)` (the top edge bows, the bottom stays)
  - `lower`: `y′ = y + b·(h/2)·p(c)·v` (the bottom edge bows, the top stays)
  - `bulge`: `y′ = yc + (y − yc)·(1 + 0.75·b·p(c))`
  - `squeeze`: `x′ = xc + (x − xc)·(1 − 0.75·b·p(d))`
  - `inflate`: `x′ = xc + (x − xc)·(1 + 0.5·b·p(d))`, `y′ = yc + (y − yc)·(1 + 0.5·b·p(c))`
  - `flag`: `y′ = y + b·(h/2)·sin(2πu)`
  - `rise`: `y′ = y − b·(h/2)·sin(π(u − ½))`
  - `arc`: the box wrapped on a circle of radius `R = w / (π·|b|)` at angle `θ = π·|b|·(u − ½)`, so a bend
    of ±1 is a half circle and the text keeps its left-to-right order for either sign. For `b > 0` the
    centre is below the box, `C = (xc, y0 + h + R)`, a point lies at radius `r = R + h·(1 − v)` (the bottom
    edge on `R`, the top on `R + h`) and maps to `(Cx + r·sin θ, Cy − r·cos θ)`. For `b < 0` the centre is
    above, `C = (xc, y0 − R)`, `r = R + h·v` (the top edge on `R`, the bottom on `R + h`) and the point maps
    to `(Cx + r·sin θ, Cy + r·cos θ)`. Every radius lies in `[R, R + h]` with `R > 0`, so no glyph passes
    through the centre and none is mirrored. `|b| < 1e-6` is the identity.
  Then the two distortions, the same for every style: `horizontal` scales height across the box,
  `y″ = yc + (y′ − yc)·(1 + 0.75·horizontal·c)`; `vertical` scales width down it,
  `x″ = xc + (x′ − xc)·(1 + 0.75·vertical·d)`. The 0.75 and 0.5 factors keep every scale at or above 0.25,
  so no scaling style folds the box. These nine styles and their formulas are the rule; the names are
  Doctored's own.
- **WT-3 — Geometry.** Glyph outlines and decorations go through the kernel's envelope map
  (`OutlineMap::Envelope`, TP-7), so glyphs ARE deformed, at a tolerance of 0.05 px divided by the layer
  transform's larger scale. Colour and bitmap glyphs are refused under a warp with a sentence.
- **WT-4 — The raster rect** is `dr_outline_map_bounds` of the layout rect — each style's bounding box in
  closed form, written out in the kernel's module doc (for `arc`, the annular sector's corners and the outer
  arc's extremes) — plus the usual padding: a pure function of the payload and the face's metrics.
- **WT-5 — UI.** Layer ▸ Warp Text…: a `WarpTextSheetController` on `makeSheetView` (a Style popup with None
  and the nine styles; Bend, Horizontal and Vertical sliders −100…100). Its preview re-renders the layer into
  `canvas.previewImage` when a control commits (not per tick) and is cleared on Cancel; OK runs
  `edit_text_layer` with `warp` through the agent dispatch as one step and records it. Offered wherever
  Replace Missing Fonts' selector is.
- **WT-6 — MCP.** `add_text_layer` and `edit_text_layer` take `warp` (the object, or `null` to remove);
  `get_document`'s text object and the replies report it.
- **WT-7 — Editing.** As TP-8: the in-place editor shows the block unwarped while it is open, and every
  payload rebuild carries `warp` over.

## Proof a build owes

- The kernel's `core/tests/outline_map.rs` checks each style against its formula on dense samples (every
  output vertex on the true mapped curve, every true sample within tolerance of the output) and `10⁵`
  interior points inside `dr_outline_map_bounds`.
- `scripts/verify_warp_text.py`: for `arch` and `flag` (pure per-column shifts) each inked column's centroid
  moves by the formula's shift within 0.75 px; for `squeeze` the same by rows; `arc` at `b = 1` puts the ink in
  a half annulus, and at `b = −1` on a short string keeps the ink's left-to-right order with no ink inside
  radius `R`; `warp: null` and all-zero equal the unwarped layer and store no key; one undo step; reopen equal
  in RGB 8/16/32, Gray, Lab and CMYK; refusals (an unknown style, a number past ±1, `on_path` present). Red on
  a build that ignores `warp`, and under mutants (the bend's sign, decorations left unwarped).
- A jobs-probe case for the sheet (one recorded `edit_text_layer` with the chosen warp; Cancel leaves no step
  and clears the preview; validation in every mode row).

## Not decided here

Warping shape layers, and a warp of colour glyphs, are limits of this design, not open work.
