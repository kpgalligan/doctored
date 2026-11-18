# Per-subpath operations held on a path (ADR)

- **Status:** Accepted; slices 1, 2 and 4 shipped (the kernel, the model and DRDC word 116, then the app,
  MCP and the options bar). Slice 4 was built on `main` a80901d2, where slice 3 (the codecs) had not
  landed: `psd_path_records::encode` still refuses a shape with `ops`, so slice 4 reads none of slice 3's
  code. Slice 3 and the owner's hand check below are what remain of
  [#19](https://github.com/kpgalligan/doctored/issues/19)'s item; "What a build owes" is the plan.
- **Date:** 2026-11-16
- **Extends:** [v1-paths.md](v1-paths.md) (its `OPEN` item for #19).

## Context

A path fills by ONE rule over all its subpaths: `PathShape.fill_rule`, nonzero or even-odd, applied to the
winding the subpaths make together (`path_geometry`). A PSD or TIFF path, and a layer's `vmsk`, combine
their subpaths instead by an operation word per subpath (`psd_path_records`: 0 exclude, 1 combine,
2 subtract, 3 intersect). Doctored reads every-0 as even-odd and every-1 as nonzero and reports any other
mix (`Decoded::combined`): a saved path keeps its outline under nonzero with a loss sentence; a `vmsk` is
not carried as a live vector mask (`psd_paths::vmsk_shape`), so a shape layer keeps its pixels and a
raster layer's mask is baked (`psd_vector_mask_bake`: each subpath drawn alone, combined per pixel by
`vector_mask_combine`). In the app, Combine Selected Subpaths and `combine_paths` produce new geometry
(`PathCombine.swift`); nothing holds an operation.

This ADR decides how a path holds the operations, so a subtracted or intersected outline stays editable
and such a `vmsk` is carried live.

The v1 ADR recorded a quarter-pixel seam wherever two subpaths share an edge. That figure is what the bake's
per-pixel combine of each subpath's OWN anti-aliased coverage gives (case (b) below). The rasterizer itself
leaves none.

## The measurement

`core/tests/path_shared_edge.rs`, through `dr_path_coverage`, anti-aliased, a 24 × 32 window from canvas
pixel (0, 0); each bound is declared in the test before anything is measured. Command:
`make t F=path_shared_edge A=--nocapture` (the four tests pass and print the worst sample).

| Case | Bound | Measured |
|---|---|---|
| (a) rectangles `[4, 10.5] × [3, 28]` and `[10.5, 17] × [3, 28]` as two subpaths of one shape, against the one rectangle `[4, 17] × [3, 28]`: nonzero and even-odd, the two orientations the same and opposite | column 10, rows 3–27, all 65535; every sample equal to the one rectangle's | column minimum 65535 and worst difference 0, in all four |
| (b) the same two rectangles, each rasterized alone, combined per pixel by `vector_mask_combine`'s formulas (union `c + s − c·s`, subtract `c·(1 − s)`) | union 49151 ± 1, subtract 16384 ± 1, the left rectangle alone 32768 ± 1, on column 10's 25 samples | 49152, 16384 and 32768 on every sample |
| (c) the two triangles of `[4.25, 19.75] × [3.5, 27.5]` split corner to corner, against the rectangle; nonzero and even-odd | within 1 | worst difference 0 |
| (d) an S-shaped cubic from `(12, 3)` to `(12, 29)` (controls `(20, 10)`, `(4, 22)`), walked forward by one closed subpath and backward by the other, against the outer rectangle `[3, 21] × [3, 29]`; nonzero and even-odd | within 64 | worst difference 0 |

Why (a), (c) and (d) hold: `artwork_coverage`'s interval walk (`accumulate_interval`) applies the fill
rule to each RUN between two consecutive crossings of an interval and spans the run at its exact x. Two
subpaths' crossings on a shared edge coincide (or differ by rounding), so the run between them is empty or
vanishingly thin, and each part of a pixel on the edge is spanned by its own run. In (a) the two loops'
x-extents only touch, so they are two clusters, each adding its exact half to the same pixel. (b) is the
seam: two independent half coverages of one pixel cannot tell disjoint halves from overlapping ones, and
`c + s − c·s` assumes independence, so the edge pixel loses a quarter.

These rows measure the EXISTING walk at coincident crossings, which is what a held-operations routine
inherits (HS-2); they do not measure that routine, which does not exist yet. The build's first red/green is
this file extended to it. With no seam to accept, no choice is left for the owner, so the status is
Accepted.

## Decisions

- **HS-1 — Model.** `PathShape` gains `ops: Option<Vec<SubpathOp>>`, one of `unite`, `subtract`,
  `intersect`, `exclude` per subpath; `SubpathOp` moves to `path_geometry` as its one home, and
  `vector_mask_combine` and `psd_path_records` use it from there.
  - **Absent**, the shape means what it means today (the subpaths' windings summed, under `fill_rule`), and
    its JSON is byte for byte today's: no key is written. Every shape in every existing file is absent.
  - **Present**, each subpath's own region is taken under `fill_rule` (its own winding alone), and the
    regions fold in order from nothing: `R ∪ S`, `R \ S`, `R ∩ S`, `R ⊕ S`; a first subpath that subtracts
    starts the fold from everything (`vector_mask_combine`'s rule, which the PSD files use), and any other
    first operation starts from nothing, so a first `unite` is that subpath's region.
  - Present with every operation `unite` is NOT absent (a counter-wound hole is filled by the union and
    cut by the summed winding), so the two are distinct values and both round-trip.
  - JSON: when the shape holds `ops`, every subpath object carries `"op"`; a shape some of whose subpaths
    carry it and others do not is refused, naming the first subpath without one. `ops.len()` equals the
    subpath count by construction: `from_value`, `refusal` and `validated` check it, the maps (`mapped`,
    `transformed`) keep it, and every edit that adds, removes or reorders subpaths (the Pen, Path
    Selection's delete and ⌥-drag copies, `set_path`) carries each subpath's operation with it; a subpath
    the Pen starts takes `unite`.
- **HS-2 — Coverage.** One kernel routine, `rasterize_ops` in `artwork_ops_coverage` beside
  `artwork_coverage` (its own module by the size rule), taking the verbs, the points and one operation per
  subpath, and sharing `outline`'s walk (`outline_by_subpath`), `edge_breaks`, the pair stage
  (`artwork_pair_sweep`), `span` and `emit_row`.
  - **Subpath index.** `PathShape::commands` opens every subpath with one move and ends it with one close,
    and `outline` tags a new loop at each of both (so loop indices are `2k + 1` today); the routine counts
    moves for the subpath index rather than relying on that arithmetic.
  - **The walk.** Its interval walk keeps one winding per subpath instead of one sum. Between two
    consecutive crossings no subpath's winding changes, so each subpath's inside/outside is constant on the
    run and so is the fold; the routine spans exactly the runs where the fold holds. The folded region
    restricted to an interval is therefore a union of runs, integrated exactly as the single winding is
    today, and a shared edge bounds an empty run, as the measurement shows for the existing walk.
  - **One cluster per row.** The x-clusters' exactness argument (`artwork_coverage`'s EXACTNESS) rests on
    a loop contributing winding only inside its x-extent and the row's area being the SUM of the clusters'.
    A fold is not a sum: an intersect clears what lies outside its subpath, and a subtracting first subpath
    covers the whole row. So the routine runs each row as ONE cluster, the reference body's shape, and a
    shape without `ops` keeps `rasterize` and its cost unchanged.
  - **Rows a subpath misses.** A subpath with no active edge in a row is outside everywhere on it, so it
    folds as a constant: `unite`, `subtract` and `exclude` of nothing change nothing, and `intersect` with
    nothing clears everything folded before it. The fold per run therefore visits only the subpaths present
    in the row, after a per-row constant prefix.
  - **Caps.** The subpath bound stays `MAX_SUBPATHS` (4,096). The work counter (`charge`, `MAX_SCAN_WORK`)
    takes the one-cluster row body's pair and interval terms plus one per present subpath per run, so a
    shape with `ops` the counter refuses refuses with today's sentence; nothing a shape without `ops` costs
    changes. The hard edge (`anti_alias` false) is the same coverage cut at one half.
  - **Proof the build owes:** `path_shared_edge.rs` extended to the routine: (a)'s two rectangles united
    equal the one rectangle, the left with the right subtracted from it is the left rectangle's own plane,
    the two intersected cover nothing, and excluded they equal the union; (c) and (d) under `unite` equal their outlines;
    each against the bounds above, and red against a fold done per pixel ((b)'s 49152 where the routine
    must give 65535).
- **HS-3 — Callers.** Every caller of `PathShape::coverage` and of `vector_mask_combine` by path:
  - `ffi_path.rs` (`dr_path_coverage`: `path_to_selection`, `fill_path` and the app's path coverage),
    `doc_vector_mask.rs` (the byte vector mask's bake) and `precision_vector_mask.rs` (the typed one) call
    `PathShape::coverage`, which the build makes dispatch to HS-2's routine when `ops` is present, so all
    three follow with no change at the call site.
  - An UNBOUNDED shape (first operation subtract and none intersect, so its fold holds with every subpath
    outside; a later intersect bounds it) bakes over the whole mask extent, its slot's outside
    coverage 255 when no enabled raster part multiplies the bake (`vector_mask_common::slot_outside`), so
    a later canvas or layer growth reads the shape. Beside an enabled raster part the slot keeps 0: beyond
    the plane the product is the raster part's effective coverage, exactly 0 when the plane is the
    raster's own rectangle and no constant otherwise, so a growth there shows 0 until the next rebake
    (under [#19](https://github.com/kpgalligan/doctored/issues/19)).
  - `landmark_region.rs` builds its own shapes, which never hold `ops`: unchanged.
  - `psd_vector_mask_bake.rs` draws each subpath as its own shape and folds them with
    `vector_mask_combine`'s per-pixel product rule. The build replaces that with one coverage of the shape
    with `ops` only if `core/tests/psd_authored_exchange.rs`'s comparison of every 16-bit bake with the
    8-bit file's rendering, within one 8-bit step, still holds; the build measures it, and keeps the product
    rule (and this decision's text) if it does not.
  - A path SHAPE layer's fill does not go through `PathShape`: the app hands `dr_artwork_path_coverage`
    the outline's verbs and points and one `even_odd` flag (`NativeArtwork.shape`). Holding `ops` on a path
    shape needs that export to take the operation list (the FFI lockstep) and HS-2's routine behind it.
    Shipped (slice 4): `ops`/`op_count`, one code per MOVE verb (0 unite, 1 subtract, 2 intersect,
    3 exclude), `op_count` 0 being today's call byte for byte (`core/tests/artwork_path_ops.rs`). The fill
    part of a holding outline is built from `ShapeOutline.foldPath()` (`PathShape.foldCGPath`), never
    `cgPath()`: CoreGraphics drops a one-anchor open subpath's lone move before another move, so it is
    written move plus a zero-length line, one move per payload subpath, its codes `opCodes`.
  - **An edgeless subpath** (one anchor without a loop handle, or any move that draws no edge) is still
    a subpath and takes its code: it is outside everywhere, so unite, subtract and exclude by it change
    nothing and intersect by it clears the fold before it. The core's walk, the app's preview fold
    (`PathCombine.foldedRegion`) and the catalog say the same; a holding shape with one renders as
    `fill_path` of the same subpaths.
- **HS-4 — Strokes.** `stroke_path` follows each subpath's flattened outline (`path_flatten`) and ignores
  `ops`: a stroke runs along the subpaths, not around the folded region. A path SHAPE layer with `ops` and
  a stroke is refused by name, since its stroke outline (`ShapeLayer.strokeOutline`) would trace subpaths
  the fold removed; Combine Selected Subpaths makes one outline first. Shipped as
  `ShapeLayerPayload.heldOpsRefusal`, which also refuses an unbounded holding outline (a shape's box cannot
  hold everything outside its subpaths): `add_shape_layer` and `edit_shape_layer` refuse by its sentence
  (whichever side the call brings, the stroke or the operations), the canvas commits (an outline gesture,
  the re-edit session, Convert to Shape Layer) show it as an alert, and a payload it names decodes as plain
  pixels.
- **HS-5 — Persistence.** The `PATH` table's entry JSON (paths and live vector-mask records alike) carries
  each subpath's `op`. A document whose paths and vector masks hold no `ops` writes the prior writer's
  bytes. One the core sees holding `ops` takes DRDC conditional word 116 (`subpath_ops_native`),
  written only when some path or live vector mask holds `ops`, so an older
  build refuses the file instead of reading the outline under one rule. A path SHAPE layer's outline lives
  in the shape payload, which the core never parses (opaque `meta`), so it needs no DRDC word: a path shape
  may hold `ops` only in the next shape payload version (8), which an older build reads as plain pixels.
  Shipped: a path payload is version 8 exactly when its outline holds `ops` (each subpath's `op` in the
  outline, omitted otherwise, so a version-7 payload keeps its bytes); 7 with an `op`, 8 without one on
  every subpath, and an unknown name decode as plain pixels.
- **HS-6 — PSD and TIFF.** `psd_path_records::decode` keeps reading every-0 as even-odd and every-1 as
  nonzero with no `ops` (every file that imports today imports the same); any other mix reads into `ops`
  (0 exclude, 1 unite, 2 subtract, 3 intersect) instead of setting `combined`, with each subpath's region
  under nonzero. `encode` writes each subpath's word from `ops`, and a shape without `ops` writes today's
  bytes. `psd_paths::vmsk_shape` then carries a mixed-operation `vmsk` live (inside `MaskRoom`'s anchor
  cap); an initial fill record of 1 is not HS-1's model and keeps today's handling (not carried; a raster
  layer's mask baked, a saved path's loss sentence). The flat TIFF path tag (34377) shares the records, so
  it follows. Checked against the `authored_exchange` files on disk, whose 8-bit twins carry a rendering of
  each mask; whether the application that wrote them reads a Doctored-written mixed-operation `vmsk` back
  as live needs a new take, which no batch may spend, so the build ships without that claim and says so.
  - Until slice 3, `encode` refuses a shape with `ops`: a saved path holding them is not written to a PSD,
    whose export report names it with its own sentence, nor to a flat TIFF, which keeps its "paths" loss;
    a vector mask holding them is written as its baked pixels.
- **HS-7 — Surface.** Every MCP path tool that takes `subpaths` (`set_path`, `set_vector_mask`,
  `path_to_selection`, `fill_path`, `stroke_path`, and `add_shape_layer` / `edit_shape_layer` with kind
  `path`) takes `subpaths[i].op`, and `list_paths`, `get_vector_mask` and `get_shape_layer` read it back; a
  shape without `ops` reads back without the key. The Path Selection options bar sets the selected
  subpaths' operation: on a path a render-neutral path edit (`ImageDocument.applyPathEdit`, one undo step,
  recorded as `set_path` with the whole shape), on a vector mask its rebake (`set_vector_mask`), on a path
  shape's outline its re-render (`edit_shape_layer`). Combine Selected Subpaths keeps producing geometry;
  its result holds no `ops`.
  - Shipped (slice 4): the app parses and writes `op` in `PathShape(subpathsJSON:)` / `subpathsJSON`, all
    or none, so every path tool takes and reads it with no per-tool parsing; `stroke_path` ignores it. The
    options bar's Operation popup (`EditorViewController+SubpathOps.swift`: —, Unite, Subtract, Intersect,
    Exclude; enabled as Combine is) records "Set Subpath Operation", the selection kept; a path that held
    none starts holding them, every other subpath `unite`. On a holding shape Combine Selected Subpaths and
    `combine_paths` with `subpath_indices` give the listed subpaths the operation and return the whole fold
    as geometry, holding none; an unbounded fold refuses (`PathCombine.unboundedRefusal`). Fill Path…,
    Stroke Path… and Make Selection… on selected subpaths of a holding path take them without `ops`.
    Checks: `scripts/verify_path_subpath_ops.py` (RGB 8, 16 and 32) and the jobs-probe's "path subpath
    operations".

## What a build owes

Ordered slices, each with its own check, each landing green before the next:

1. **Kernel and test.** `SubpathOp` homed in `path_geometry`; HS-2's routine; `path_shared_edge.rs`
   extended as HS-2 says, seen red against a per-pixel fold. No model, FFI or app change.
2. **Model and DRDC word.** `PathShape.ops`, its JSON and refusals, `PathShape::coverage`'s dispatch, the
   `PATH` table carrying it and the conditional word (HS-5). Checks: a black-box round trip through
   `dr_path_*` and the native save; a document with no `ops` written byte for byte as the prior writer's;
   the byte and typed vector-mask bakes of a shape with `ops` against HS-2's routine.
3. **Codecs.** HS-6's decode and encode, `vmsk_shape` carrying a mixed-operation `vmsk`, and HS-3's
   measured choice for `psd_vector_mask_bake`. Checks: `core/tests/psd_paths.rs` and
   `psd_authored_exchange.rs` against the files on disk; every file that imports today imports the same.
4. **App, MCP and drive.** `subpaths[i].op` parsing and read-back, the shape payload's version 8 and the
   stroke refusal (HS-4), the Path Selection options bar's control and its recorded steps, a colour mode's
   `tools` set where it lists the path tools, and the generated MCP reference. Check: a drive over MCP at
   8, 16 and 32 bits (each operation's coverage against `dr_path_coverage` of the equivalent combined
   path, save and reopen, undo, a replayed recorded step), plus jobs-probe rows for the options bar.

Hand check (the owner's, still owed): the options bar's four operations on a real path, a vector mask and
a path shape in a real window.
