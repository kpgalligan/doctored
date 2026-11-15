# Type on a path (ADR)

- **Status:** Accepted, not built — an item of [#19](https://github.com/kpgalligan/doctored/issues/19). The
  decisions below are what a build follows; the code does not exist yet.
- **Shares** its geometry kernel and its payload version with Warp Text ([warp-text.md](warp-text.md)).

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
  Size keep working through the anchor and `transform` with no new code. A payload is version 6 exactly when
  it carries `on_path` or `warp` (Warp Text), in any ink, and then always carries `artwork`, whose `ink` says
  what the numbers are (the rule shape versions 5–7 use). Versions 1–5 keep their bytes and their decode guard
  (3 sRGB, 4 linear, 5 CMYK). Both keys at once is "not a text layer". No DRDC word: the description is the
  opaque per-layer meta (and the typed description table's copy of the same JSON).
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
- **TP-5 — Colour and bitmap glyphs** (emoji) are refused on a path with a sentence ("text on a path needs
  outline glyphs") unless a probe case shows the existing per-glyph plane drawn one glyph at a time with the
  rotation folded into its text matrix; whichever ships, this ADR is updated to say it.
- **TP-6 — The raster rect** is a pure function of the payload and the face's metrics, as today's is: the
  outline's control box grown by `ascender − descender + |baseline shift| + TextLayer.padding(forSize:)`,
  rounded out. The anchor recovery reads it on every open, so it never depends on a layout's measured ink.
- **TP-7 — The kernel** (shared with Warp Text). Two new KERNEL modules (`scripts/check_layers.py`):
  `path_along.rs`, arc length over the existing flattener at 1/64 px (its arc-length error bound stated in
  the module doc): length, and the frame (point, unit tangent) at a distance, a closed subpath wrapping and an
  open one answering none past its ends; and `outline_map.rs`, an outline in `artwork_coverage`'s command form
  through a non-linear point map, returned as moves, lines and closes, each segment subdivided until the
  chord is within the tolerance of the TRUE mapped curve (a straight line splits at least once, since it maps
  to a curve), refused past `MAX_PATH_POINTS`. Exports `dr_path_length`, `dr_path_frames` (a caller-owned
  buffer, the in-place write called out), `dr_outline_map` (JSON, as `dr_path_flatten` returns) and
  `dr_outline_map_bounds`, through `ffi_util`, with core tests against analytic oracles (a circle's length
  against 2πr, its frames against the analytic point and tangent).
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
  file path and exempts only top-level keys. `get_document`'s text object and both replies report `on_path`
  with `canvas_subpaths`, `offset`, `side` and `length`.

## Proof a build owes

- Core: `core/tests/path_along.rs` and `core/tests/outline_map.rs` through the FFI (above), each seen red.
- `scripts/verify_text_on_path.py`: text on a circle of radius r lies in the annulus
  `[r − descent − 2, r + ascent + 2]`, its angular span is its advance over r within 3 %; `offset` + Δ turns
  the ink's mean angle by Δ/r; `side` puts it inside; `on_path: null` equals a never-placed layer; one undo
  step each; reopen equal in RGB 8/16/32, Gray, Lab and CMYK; an open path reports `hidden_glyphs`; the
  refusals. Red on a build that ignores `on_path`, and under mutants (rotation dropped, alignment ignored).
- A jobs-probe case for the two menu items (validation in every `colourModeRows` row, one recorded step that
  replays, a text-session commit keeping `on_path`).

## Not decided here

PSD exchange keeps crossing a text layer as pixels. Curved in-place editing, a path and a warp on one layer,
and colour glyphs on a path beyond TP-5 are limits of this design, not open work.
