# Warp Text (ADR)

- **Status:** Accepted — shipped 2026-11-16, an item of [#19](https://github.com/kpgalligan/doctored/issues/19)
  (per-subpath operations stay open there).
- **Shares** the outline-map kernel (TP-7) with [type-on-a-path.md](type-on-a-path.md); its payload
  version is 8 ([text-ink-rect.md](text-ink-rect.md) TI-6).
- **Code:** core `outline_envelope.rs` (the envelope and its bounds), `outline_map.rs` (`OutlineMap::Envelope`),
  `outline_limits.rs` (the 1e9 coordinate bound), `ffi_path_along.rs` (`dr_outline_map`'s envelope map,
  `dr_outline_map_bounds`); app `TextWarp.swift` (the value, `withWarp`, the WT-4 rect),
  `ArtworkTextWarp.swift` (the capture's envelope), `DoctoredCore+PathAlong.swift`,
  `AgentServer+TextWarp.swift`, `WarpTextSheetController.swift`, `EditorViewController+WarpText.swift`.

## Decisions (WT-1..WT-7)

- **WT-1 — The description.** `TextLayerPayload` version 8 adds an optional `warp`: `{"style", "bend",
  "horizontal", "vertical"}`, all four written, each number in −1…1. A payload carrying it is version 8, in
  any ink, always with `artwork` and a stored `box_width`; `ink_outset` may ride along (TI-6). All three at 0
  is no warp and is never stored: the tools remove the key, and an edit that changes nothing registers no
  step. `warp` and `on_path` on one layer is "not a text layer", as is a `warp` below 8, an 8 without it, or
  an invalid or all-zero `warp`; a tool that sets one while the other is present refuses and names it.
  Versions 1–7 keep their bytes.
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
    to `(Cx + r·sin θ, Cy + r·cos θ)`. Every point inside the box lies at a radius in `[R, R + h]` with
    `R > 0`, so no glyph passes through the centre and none is mirrored. `|b| < 1e-6` is the identity.
  Then the two distortions, the same for every style: `horizontal` scales height across the box,
  `y″ = yc + (y′ − yc)·(1 + 0.75·horizontal·c)`; `vertical` scales width down it,
  `x″ = xc + (x′ − xc)·(1 + 0.75·vertical·d)`. The 0.75 and 0.5 factors keep every scale at or above 0.25,
  so no scaling style folds the box. These nine styles and their formulas are the rule; the names are
  Doctored's own.
  **Outside the box** (a glyph's ink and the padding overhang the layout rect): the fractions inside `p(·)`,
  `sin(·)`, the `(1 − v)` / `v` weights and both distortion factors are clamped to the box while `x`, `y` in
  the formulas are not. `arc`, stated for `b > 0` (mirrored for `b < 0`): `θ` is taken at `u` clamped; on the
  outer side the radius is `R + h·(1 − v)` with `v` unclamped; on the inner side, overhang `e = h·(v − 1)`,
  it is `R² / (R + e)` — `R − e` to first order at the edge, decreasing in `e` and always in `(0, R)`, so no
  point reaches the centre whatever the overhang; a point past the box's left or right end adds its signed
  residual along the tangent at that end's angle.
- **WT-3 — Geometry.** Glyph outlines and decorations go through the kernel's envelope map
  (`OutlineMap::Envelope`, TP-7), one call per glyph so overlapping glyphs keep their order, so glyphs ARE
  deformed, at a tolerance of 0.05 px divided by the layer transform's larger scale. Colour and bitmap
  glyphs are refused under a warp: "warped text needs outline glyphs: remove the colour or bitmap glyphs
  (emoji), or remove the warp". The capture's 262,144-point cap is unchanged.
- **WT-4 — The raster rect** (as built) is `dr_outline_map_bounds` of the envelope over the layout rect,
  taken of the straight source rect (the layout rect padded by `TextLayer.padding(forSize:)` and grown by any
  `ink_outset`), grown by 1 px (`TextWarp.margin`): a pure function of the payload and the face's metrics.
  The bounds are exact without distortions: per axis, the map is evaluated on a grid of the region's ends,
  the clamp breakpoints and each style's critical fractions (`u = ½` for `p(c)` and `arc`'s `θ = 0`, `¼` and
  `¾` for `flag`, `v = ½` for `squeeze`'s and `inflate`'s widest row); with distortions, the interval
  product with each factor's range covers it. `outline_envelope.rs`'s module doc writes this out per style.
  `ink_outset` is measured on the unwarped layout, the straight overflow, whose image the rect covers.
- **WT-5 — UI.** Layer ▸ Warp Text…: a `WarpTextSheetController` on `makeSheetView` (a Style popup with None
  and the nine styles; Bend, Horizontal and Vertical sliders −100…100, off at None). Its preview re-renders
  the layer into `canvas.previewImage` when a control commits (not per tick; on a worker for a high-depth
  document) and is cleared on Cancel and OK; OK runs `edit_text_layer` with `warp` through the agent
  dispatch as one "Warp Text" step and records it, and an unchanged choice records nothing (controls left as
  the sheet opened keep a stored warp off the sliders' whole-hundredth grid, never round it; the test is the
  raw controls, so None still removes a warp that rounds to 0). Offered with no
  Quick Mask on a text layer off a path, wherever Replace Missing Fonts' selector is (the float, 16-bit,
  Gray, Lab and CMYK lists); Place Text on Path is off for a warped layer.
- **WT-6 — MCP.** `add_text_layer` and `edit_text_layer` take `warp` (`{style, bend?, horizontal?,
  vertical?}`, or `null` to remove; in `edit_text_layer` an omitted style or number keeps the layer's);
  `get_document`'s text object and the replies report it. A call passing only `warp` that leaves the stored
  one as it is changes nothing and leaves no step.
- **WT-7 — Editing.** As TP-8: the in-place editor shows the block unwarped while it is open, and every
  payload rebuild carries `warp` over (the text commit, `edit_text_layer`); Replace Missing Fonts recentres a
  warped layer on both axes.

## Proof

- Core `core/tests/outline_envelope.rs` through the FFI, against WT-2 written in the test: each style at
  bends ±0.5, ±1 and one distorted row, over an outline inside the box and one overhanging it, every output
  vertex the oracle's image of a point of its segment (1e-9) and dense oracle samples within the tolerance
  (twice it across the clamp's corner for the overhang); 10⁵ interior points of the box and of a region 20 %
  larger inside the bounds, every side met by the boundary; `squeeze` b −1 and `inflate` b 1 at
  `[−37.5, 137.5]` and `[−25, 125]`; `arc`'s band, order and inner overhangs (2R, and a one-glyph box under a
  wider padding); refusals. Red under mutants: the bend's sign in `arch`, `flag`'s 2π halved, a distortion
  factor unclamped, `v = ½` dropped from the grid, `arc`'s inner rule as `R − e`.
- `scripts/verify_warp_text.py` in RGB 8/16/32, Gray, Lab and CMYK: `arch` and `flag` move each column's
  centroid by the formula, `squeeze` scales each row's (0.75 px); `arc` b 1 inks the half annulus and b −1
  keeps "AB" in order off the centre; an underline follows the arch; `null` and all zero equal the
  never-warped layer and store no key; one step each; reopen and Replace Missing Fonts equal; refusals; and
  (RGB 8) a warped Zapfino swash leaves its raster ring clear. Red on a build whose capture ignores `warp`
  and under mutants (the bend's sign, decorations left straight, `edit_text_layer` dropping `warp`).
- jobs-probe "warp text": validation in every `colourModeRows` row and off on a path; the sheet's preview,
  Cancel, one recorded step that undoes and replays, an unchanged OK (an off-grid MCP bend of 0.125 too),
  None (on a bend of 0.004 too), a session commit keeping the warp; the codec (versions 1–7 byte for byte, 8 round-trips, malformed 8s refused, a one-anchor subpath
  refused). Red under mutants (Cancel keeping the preview, the session dropping `warp`, the decoder taking
  `warp` at 7).
- Hand check (the owner's): the sheet's preview by eye in a real window; each style on real fonts (a script
  face, a multi-line paragraph); the straight in-place editor on a warped layer.

## Not decided here

Warping shape layers, a warp of colour glyphs, a path and a warp on one layer, curved in-place editing and
PSD exchange of the warp are limits of this design, not open work.
