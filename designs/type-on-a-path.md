# Type on a path (ADR)

- **Status:** Accepted — shipped 2026-11-16, an item of [#19](https://github.com/kpgalligan/doctored/issues/19)
  (Warp Text and per-subpath operations stay open there).
- **Shares** its geometry kernel with Warp Text ([warp-text.md](warp-text.md)).
- **Code:** core `path_along.rs`, `outline_map.rs`, `ffi_path_along.rs`; app `TextOnPath.swift` (the value,
  the source rect, the metrics), `ArtworkTextOnPath.swift` (the placement the capture asks),
  `DoctoredCore+PathAlong.swift`, `AgentServer+TextOnPath.swift`, `EditorViewController+TextOnPath.swift`.

## Context

A text layer's ink is glyph OUTLINES: `TextArtworkLayout.showCGGlyphs` (`ArtworkTextCapture.swift`) maps each
glyph's `CTFontCreatePathForGlyph` outline from layout space into raster space and hands it to the core as a
`.path` part (`dr_artwork_path_coverage`); underline and strikethrough arrive as paths in raster space too.
Colour and bitmap glyphs are drawn by TextKit into byte planes. So one geometric stage between "glyph in
layout space" and "raster space" is where text on a path belongs, and nothing downstream changes. The core
has the path model and its flattener (`path_flatten.rs`), but no arc length and no non-linear outline map.

## Decisions (TP-1..TP-9)

- **TP-1 — The description.** `TextLayerPayload` version 6 adds an optional `on_path`, written only when
  present: `{"outline": <ShapeOutline, exactly one subpath, at most 1,024 anchors>, "offset": px, "side":
  "left" | "right"}`, in the layer's SOURCE space (relative to the anchor), so Move, Free Transform and Image
  Size keep working through the anchor and `transform` with no new code. A payload carrying `on_path` is
  version 6 (a version is the highest feature a payload carries: 7 `ink_outset`, 8 Warp Text's `warp`,
  [text-ink-rect.md](text-ink-rect.md) TI-6), in any ink, and then always carries `artwork`, whose `ink` says
  what the numbers are. Versions 1–5 keep their bytes and their decode guard (3 sRGB, 4 linear, 5 CMYK).
  `on_path` and `warp` at once is "not a text layer". No DRDC word: the description is the opaque per-layer
  meta (and the typed description table's copy of the same JSON).
- **TP-2 — One line.** A text on a path is point text; a string with a line break is refused. TextKit lays the
  line out straight as today (kerning, tracking, runs); each glyph is then placed RIGIDLY: the x of its
  advance's centre gives a distance `s = start + x` along the subpath, the frame at `s` gives a point and a
  unit tangent, and the glyph outline is rotated to the tangent about its centre on the baseline. Glyphs are
  not deformed. `start` follows the layer's alignment: left starts at `offset`, center is centred on it, right
  ends at it. `side: "right"` runs the text on the other side of the path (the frame reversed). Baseline shift
  moves along the normal.
- **TP-3 — The ends.** On an open subpath a glyph whose centre has no frame is not drawn (text past an end is
  cut off) and the replies report `hidden_glyphs`; a closed subpath wraps.
- **TP-4 — Decorations** (underline, strikethrough) follow the path through the kernel's outline map
  (TP-7), so they bend where the glyphs turn.
- **TP-5 — Colour and bitmap glyphs** (emoji) are refused on a path: "text on a path needs outline glyphs:
  remove the colour or bitmap glyphs (emoji), or remove the text from the path" (the capture throws it, so
  every render path refuses alike).
- **TP-6 — The raster rect** is a pure function of the payload and the faces its layout resolves, as the
  straight rule is: the outline's control box grown on every side by `hypot(A/2, ascender − descender) +
  |baseline shift| + TextLayer.padding(forSize:)`, with `A` the widest glyph advance of the straightened
  line, each read from the face its run was typeset in (so a fallback face counts). A rigidly placed glyph
  reaches half its advance along the tangent and its height across from a point of the curve, which lies in
  the control box. The anchor recovery reads it on every open, so it never depends on a layout's measured
  ink. Its centre is face-free, so Replace Missing Fonts recentres such a layer on both axes.
- **TP-7 — The kernel** (shared with Warp Text). Two new KERNEL modules (`scripts/check_layers.py`):
  `path_along.rs`, arc length over the existing flattener at 1/64 px (its arc-length error bound stated in
  the module doc): length, and the frame (point, unit tangent) at a distance, a closed subpath wrapping and an
  open one answering none past its ends; and `outline_map.rs`, an outline in `artwork_coverage`'s command form
  through a non-linear point map, returned as moves, lines and closes, each segment subdivided until the
  chord is within the tolerance of the TRUE mapped curve (a straight line splits at least once, since it maps
  to a curve), refused past `MAX_PATH_POINTS`. Exports `dr_path_length`, `dr_path_frames` (a caller-owned
  buffer, the in-place write called out), `dr_outline_map` (JSON, as `dr_path_flatten` returns), through
  `ffi_util`, with core tests against analytic oracles (a circle's length against 2πr, its frames against
  the analytic point and tangent). `dr_outline_map_bounds` and `OutlineMap::Envelope` arrive with Warp Text,
  their only caller.
- **TP-8 — Editing.** The in-place text editor opens a placed layer as it does today and shows the line
  STRAIGHT while it is open; the commit renders it on the path. Every site that rebuilds a payload through
  `TextLayerPayload.init(string:style:…)` carries `on_path` over (the text commit, `edit_text_layer`, Replace
  Missing Fonts, the CMYK repaint).
- **TP-9 — UI and MCP.** Layer ▸ Place Text on Path (the active layer a one-line text layer and a non-empty
  current path; the first selected subpath under the selection tools, else the path's first; offset 0) and
  Layer ▸ Remove Text from Path, each one undo step run through the agent dispatch and recording
  `edit_text_layer`, offered wherever Replace Missing Fonts' selector is (the float, 16-bit, Gray, Lab and
  CMYK lists). `add_text_layer` and `edit_text_layer` take `on_path: {subpaths | path_name, subpath?, offset?,
  side?}` (canvas px; exactly one subpath once `subpath` picks it) or `null` to remove. The stored path's name
  is `path_name`, not `path`: `AgentPathGuard.catalogFailure` reads a property named `path` at any depth as a
  file path and exempts only top-level keys (`on_path` itself is exempted there by name, as it ends in
  `_path`). `get_document`'s text object and both replies report `on_path` with `canvas_subpaths`, `offset`,
  `side`, `length` (source px) and `hidden_glyphs`. As built: an `edit_text_layer` object with neither
  `subpaths` nor `path_name` keeps the stored outline and changes only `offset` / `side`; with `on_path`,
  `add_text_layer`'s `x` and `y` default to the subpath's first anchor. The UI names a list path by
  `path_name` and `subpath`, a vector mask or path shape outline by its one `subpaths` entry. A text-session
  commit keeps the layer's path and refuses a line break with the session left open.

## Proof

- Core: `core/tests/path_along.rs` and `core/tests/outline_map.rs` through the FFI, seen red under mutants
  (closed not wrapping, tangent left unnormalised, `reversed` ignored, a line not split).
- `scripts/verify_text_on_path.py`: text on a circle of radius r lies in the annulus
  `[r − descent − 2, r + ascent + 2]`, its angular span is its advance over r within 3 %; `offset` + Δ turns
  the ink's mean angle by Δ/r; alignment moves it by half and the whole advance; `side` puts it inside; an
  underline stays in the annulus; `on_path: null` equals a never-placed layer; one undo step each; reopen and
  Replace Missing Fonts equal in RGB 8/16/32, Gray, Lab and CMYK; an open arc reports `hidden_glyphs`; the
  refusals; two wide glyphs uncut on a circle's top and at a line's end: Zapfino's "Zapfino" ligature and a
  fallback glyph (U+FDFD, else U+2E3B), the one whose box needs the `A/2` term (Zapfino's taller face holds its glyph
  without it). Red on a build that ignores `on_path`, and under mutants (rotation dropped, alignment ignored,
  decorations left straight, `on_path` not carried by `edit_text_layer`, the `A/2` term dropped).
- jobs-probe "text on a path": both items' validation in every `colourModeRows` row, one recorded step each
  that replays, a text-session commit keeping `on_path` and its line break refused.
- Hand check (the owner's): Place Text on Path on a drawn path by eye — spacing on tight curves, the straight
  in-place editor on a placed layer, both items' enablement.

## Not decided here

PSD exchange keeps crossing a text layer as pixels. Curved in-place editing, a path and a warp on one layer,
and colour glyphs on a path beyond TP-5 are limits of this design, not open work.
