# Paths, the Pen tool and vector masks (ADR)

- **Status:** Accepted — open items remain ([#13](https://github.com/kpgalligan/doctored/issues/13),
  [#19](https://github.com/kpgalligan/doctored/issues/19))
- **Date:** 2026-10-11
- **Closes the track items:** "Pen/Bézier node editing" and "path operations/vector masks" of *Text and paths* in the
  [plan](professional-photo-editor-implementation-plan.md)

## Context
The app had selections, shape layers and raster layer masks but no path: no way to draw an exact outline,
keep it, and turn it into a selection, a fill, a stroke or a mask. This batch adds paths as document state at
8, 16 and 32 bits, a Pen tool, a Paths panel, the four path operations, vector masks and MCP mirrors.

## Decisions
- **Model.** A `PathShape` is subpaths of anchors plus a fill rule (`nonzero` / `evenodd`). An anchor has a
  point, two absolute handles and a `smooth` flag; segment a→b is the cubic `(a.point, a.handle_out,
  b.handle_in, b.point)`, a line when both handles sit on their points. An open subpath is closed for fill
  only. Coordinates are canvas pixels quantized to four decimals (`style_json::q4_f64`), within ±1,000,000.
  The kernel is `core/src/path_geometry.rs`; it names no document type.
- **The list.** A document holds up to 256 paths (`doc_path.rs`): at most one **work path** (always first,
  always named "Work Path") and saved paths with unique names. One cap covers every path and every live
  vector-mask shape: 65,536 anchors, sized so the native file's `PATH` table JSON stays under its 16 MiB limit.
  `list_refusal` is the one home of these rules, and its sentence is what the FFI and MCP return.
- **A path edit is render-neutral.** It goes through `ImageDocument.applyPathEdit` (`applyGuideEdit`): one
  undo step, the document dirty, no reprojection. The **current path** is view state (`currentPath`), not
  document state, so there is no `select_path` tool: each tool names its path.
- **Geometry.** Paths follow crop, canvas size, image size, rotate/flip, arbitrary rotation and perspective
  crop through the same point map a colour sampler takes at that call site. They are never dropped and never
  clamped to the canvas; a coordinate that would leave ±1,000,000 lands on that wall.
- **`.doctored`** (legacy `.rz` still read). Word 74 (byte) / 75 (typed) sits inside the identity wrapper, after
  the conditional word 79 (`raw_locals_native`), and the `PATH` table
  is the last before `LIDS`. It is written only when a path or a live vector mask exists, so any other
  document is byte-for-byte the earlier writer's file. Ids are minted on load. A layered PSD, PSB or TIFF
  carries the paths (resources 1025 and 2000–2997, `psd_paths`) at every depth and mode, and a vector mask with
  no raster part as its `vmsk`, in every mode. A flat TIFF of every mode but Duotone (RGB, CMYK, Grayscale and
  Lab at 8 and 16 bits, Indexed, Bitmap, 32-bit RGB and Grayscale) carries them in tag 34377 (`tiff_paths`):
  each mode's open reads them (none from a file stored rotated, with a sentence) and takes every path resource
  out of the document's 8BIM run, so the model is their one home; each mode's TIFF writer writes the model's
  paths after the run (the 32-bit TIFF through the integer writer's sidecar tags, so it also holds the EXIF
  and 8BIM run its `carried` bits report). A trimmed layer export, a part of the canvas, writes none. Path
  resources a run still holds (a JPEG's APP13) are fractions of the raster that file stored: every JPEG open
  that turns the raster upright (EXIF Orientation not 1) drops them with a sentence; otherwise they pass
  through a TIFF of a document with no path, give way to one that has paths (a `"paths"` loss), and leave the
  run at every whole-canvas edit but Image Size (`tiff_paths::canvas_moved`), so no reopen reads them where
  they no longer belong. Any other flat save names `"paths"` among its losses, and so does a TIFF holding a
  path past 128 canvases; Indexed and Bitmap name them in their combined line, which a TIFF that keeps them
  leaves out.
- **Clipping path.** At most one SAVED path is the document's clipping path (`doc_path::ClippingPath`, on the
  entry, so rename, delete, geometry and undo carry it). It crosses PSD, PSB and every TIFF as resource 2999,
  the path's name as a Pascal string; the bytes a file holds after the name are kept verbatim and written
  back, and a designation made here writes the name alone. `.doctored` word 114 (`clipping_path_native`, no
  table, the key in the `PATH` entry) is written only when a path is the clipping path. Paths panel ▸ Clipping
  Path and `set_clipping_path`.
- **Operations**, each a core function with an MCP tool (`AgentServer+Paths.swift`):
  - coverage: `artwork_coverage::rasterize` over the path's commands, the one rasterizer shapes use;
    `anti_alias: false` cuts at half coverage. `path_to_selection` commits it with mode and feather.
  - `fill_path`: `stroke_selection`'s band commit with band = path coverage × selection.
  - `stroke_path`: the Brush or Eraser run along each flattened subpath (tolerance 0.25 px) through the
    `brush_stroke` / `eraser_stroke` handlers, all subpaths one undo step. It asks the inner tool's gates and
    the stroke's own 10,000-point limit for EVERY subpath before the first paint, so a refusal paints
    nothing. Refused on a camera RAW layer, as the Brush is. In a 32-bit float document it paints as the
    float Brush and Eraser do: `rgba32` is linear ink, and the panel passes it for a foreground above
    white on a layer.
  - `selection_to_path` (Make Work Path): `path_trace::trace`, the 50 % contour simplified by
    Douglas–Peucker and fitted with Schneider's cubics, within `tolerance` (0.5–10 px) of the contour.
- **A vector mask is a baked ordinary mask.** The record (`doc_vector_mask.rs`, typed twin
  `precision_vector_mask.rs`) keeps the shape, `enabled`, `linked` and the user's raster mask if there is
  one; the layer's mask slot holds raster coverage × path coverage. The compositor, the style cache, the
  regional renderers and PSD/TIFF export see only that baked mask and were not changed.
  - A record is **live** while its layer exists and the slot's plane is the one it baked. Anything that
    replaces the plane without going through the record leaves a plain raster mask (the picture is
    unchanged), and the dead record is pruned at the next path write and never saved.
  - Mask edits on a layer with a live record run on the raster part and rebake. Apply Layer Mask applies the
    combined coverage and removes both.
  - A move shifts the shape with the slot. Free Transform of a linked mask, rotate/flip canvas, Image Size
    and the 8↔16 conversions map the SHAPE and rebake when the record has no raster mask beside it; with
    one, the result is a plain raster mask.
  - A disabled vector mask with no raster mask keeps its plane in the slot, disabled, so it still travels
    with the layer. An empty shape (Reveal All) with no raster mask leaves no mask plane.
  - The layer-mask tools edit the raster mask only; on a vector mask with no raster mask they refuse and
    name the vector-mask tools.
- **Pen tool** (P, its own rail slot; `PenSession.swift`). Click places a corner; drag places a smooth
  anchor with mirrored handles; clicking the open subpath's first anchor closes it; Escape or Return ends
  the subpath; ⌘-drag moves an anchor or a handle; ⌥-click converts; a click on a segment inserts and a click
  on another anchor deletes. With no current path the Pen creates or replaces the work path. Each gesture is
  one undo step. `dr_path_hit` takes a `handles` flag so a plain click reaches the segment under a handle tip.
- **Paths panel**, the right panel's sixth tab: the active layer's vector mask (when it has one), the work
  path and the saved paths; rename (which saves the work path), duplicate, delete, Clipping Path, Make
  Selection…, Fill Path, Stroke Path, Make Work Path… and Add Vector Mask. Fill uses the foreground colour; Stroke uses the
  Brush tool's stored options. Layer ▸ Vector Mask has Current Path, Reveal All, Delete, Enable and Link;
  Select ▸ Make Work Path… traces the selection.
- **MCP** (fourteen tools): `list_paths`, `set_path`, `rename_path`, `duplicate_path`, `delete_path`,
  `set_clipping_path`, `path_to_selection`, `selection_to_path`, `fill_path`, `stroke_path`, `get_vector_mask`, `set_vector_mask`,
  `set_vector_mask_properties`, `remove_vector_mask`. An operation tool takes `path` (a name) or explicit
  `subpaths` + `fill_rule`. `get_document` reports a `paths` count and a per-layer `vector_mask` flag.
- **Actions.** Every UI entry records the tool step it is: the Pen records the whole shape (`set_path`, or
  `set_vector_mask` when it edits a mask), the panel and the menus record their tool with the path's NAME,
  and layer arguments are `{"$layer": "active"}`. An action that names a path therefore needs that path in
  the target document, which a recorded Pen or Make Work Path step earlier in the same action provides.

## Not in v1
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Type on a path.
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Convert Path to Shape layer (shape layers
  hold only rectangle, ellipse and line geometry).
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Path Selection and Direct Selection as
  separate tools; path boolean operations; per-path fill and stroke dialogs.
- `DONE` — paths in PSD, PSB and layered TIFF, both directions, every depth and mode (`psd_paths`,
  `psd_path_records`: Adobe's 26-byte records, saved paths by name, the work path, entry-by-entry import); at RGB
  8/16 a `vmsk` with no raster mask beside it imports as the layer's vector mask (a shape layer as its fill
  under the path) and a raster-less vector mask exports as its `vmsk` (`core/tests/psd_paths.rs`, against
  Photoshop's own `vmsk` bytes). The record walk decides which `vmsk` is carried (`psd_paths::vmsk_shape`,
  `MaskRoom`): one that starts from all pixels, subtracts or intersects its subpaths, or would take the file's
  paths and vector masks past the 65,536-anchor cap keeps the record's pixels and the dropped-mask sentence.
- `DONE` (2026-11-12) — the same `vmsk` exchange in the Gray (Duotone), Lab, CMYK and 32-bit codecs
  (`psd_typed_exchange`, `core/tests/psd_mode_exchange.rs`); a `vmsk` over a fill the mode's open does not import
  as a fill layer (every fill at Gray and Lab, a Gradient or Pattern fill at 32 bits) is not carried, so that
  shape layer keeps its stored pixels.
- `DONE` — the clipping path (resource 2999) in PSD, PSB and TIFF, and paths in a flat RGB or CMYK TIFF
  (`core/tests/psd_clipping_path.rs`, `core/tests/tiff_paths.rs`, against the published layouts computed in
  the tests; no authored 2999 is on disk).
- `DONE` (2026-11-12) — paths in a flat Grayscale, Lab, Indexed, Bitmap and 32-bit TIFF, each mode's own open,
  writer and loss list (`core/tests/tiff_paths_modes.rs`); a JPEG whose open turns it drops its APP13 path
  records with a sentence (`core/tests/tiff_paths.rs`). A JPEG's paths are still not read into the model.
- `OPEN` [#13](https://github.com/kpgalligan/doctored/issues/13) — a vector mask beside a raster mask, an
  inverted one, one whose subpaths subtract or intersect, and `vsms`; the layout of the bytes after a 2999's
  name (its flatness), which needs an authored sample.
- `DROPPED` (2026-10-30) — Curvature Pen; Freeform and Magnetic pen: the Pen covers authoring, and the Magnetic
  Lasso plus Make Work Path covers edge tracing.
- `DROPPED` (2026-10-30) — pressure or canonical-dab strokes along a path: Stroke Path ships, and simulated
  pressure has no request on record.
- `DROPPED` (2026-10-30) — a vector mask surviving warp, perspective, perspective crop, straighten, arbitrary
  canvas rotation, layer duplicate, the clipboard, a merge, or any resampling transform while it has a raster
  mask beside it: each bakes to a raster mask with the same picture, so nothing visible is lost.

## Consequences
- A feature that replaces a layer's mask plane does not need to know about vector masks: the record dies and
  the baked mask stays. A feature that should KEEP the vector mask maps the shape and rebakes
  (`following_vector_masks`).
- A host path that reads a layer mask's properties and writes them back must start from
  `RasterDocument.maskEditBaseline(_:)`, the raster mask's own values, not from the baked mask's.
- New path geometry goes in the kernel and through `artwork_coverage::rasterize`; there is one rasterizer.

## Verification
- Core: `core/tests/path_geometry.rs`, `path_shape.rs`, `path_format.rs`, `path_trace.rs`,
  `vector_masks.rs`, `precision_vector_masks.rs`, each against an oracle written in the test (per-pixel
  area, Green's-theorem area of a Bézier circle, Bernstein evaluation, a supersampled point-in-polygon
  flattener).
- App: `python3 scripts/verify_v1_paths.py` drives the Debug app over MCP: the list and undo, selections
  both ways, fill and stroke at 8 and 16 bits, `stroke_path`'s refusals, vector masks at 8, 16 and 32 bits,
  `.rz` save and reopen, and recorded Actions replayed, including the steps the Pen, the panel and the menus
  record.
- Memory: `core/examples/path_admission.rs` measures the charges of `dr_path_coverage`, `dr_path_trace` and
  the 16-bit vector-mask bake (with and without a feathered raster mask) at 24 MP near their caps (Release,
  one case per process, `MallocLargeCache=0`); each charge is at least 2.2 × the call's measured peak.
