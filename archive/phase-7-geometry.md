# Phase 7D: typed geometry

This internal batch adds geometry to the separate RGBA16 document path. It does
not enable the public high-depth editor mode; that remains the 7E integration
gate. The current representation and overall status are recorded in
[the high-depth status](phase-7-high-depth-status.md).

## Requests and targets

The strict, at-most-64-KiB request is parsed by
[GeometryEdit](../core/src/precision_geometry.rs). Unknown operations, filter
names and fields refuse before pixel allocation.

| Operation | Request fields | Scope |
| --- | --- | --- |
| `affine` | `layer`, `target`, `matrix`, `filter` | Raster leaf pixels or an existing independent mask. Matrix order is `[a,b,c,d,tx,ty]`, in canvas coordinates. |
| `perspective` | `layer`, `target`, `quad`, `filter` | The same targets; corners are source TL, TR, BR, BL destinations in canvas coordinates. |
| `resize` | `width`, `height`, `filter` | Entire supported tree, every stored raster/mask and every canvas channel. |
| `crop` | `x`, `y`, `width`, `height` | Nonempty rectangle inside the existing canvas. |
| `canvas_size` | `width`, `height`, `x`, `y` | A generalized crop (12F-1): `(x, y)` is where the OLD canvas's top-left corner lands in the new one. Every raster and mask keeps its allocation and moves its offset, every channel becomes a canvas-sized plane holding an exact copy at the origin and 0 elsewhere, guides/ruler origin follow `doc_guide::padded_*`. Lock-exempt, admitted at 16 bits AND at 32-bit float, and the ONE whole-document op a retained image source survives. |
| `straighten` | `matrix`, `filter`, `x`, `y`, `width`, `height` | The Crop tool's straighten and the agent `crop` tool's, as ONE op (12F-1b2): `matrix` turns the picture in canvas coordinates and `(x, y, width, height)` — a rectangle inside the existing canvas — becomes the new canvas. Equal to the byte composition `straighten_layers(matrix)` + `transform_channels(matrix)` + `crop`. Every raster and EVERY mask rides `matrix` composed with the window shift, every channel resamples straight into the window, chrome is `cropped_*`, the PPI is never transposed, and a resampled raster drops its description. Lock-exempt; 16 bits ONLY (a 32-bit float document refuses by name); a retained image source refuses. |
| `rotate90`, `rotate180`, `rotate270` | None | Entire document; quarter turns transpose dimensions and PPI axes. |
| `rotate_canvas` | `degrees`, `filter` | ARBITRARY canvas rotation at 16 bits (12F-1b4), planned from the SAME `doc_rotate::{rotation_kind, rotated_canvas}` as the byte `RzDocument::rotate_canvas`, so both depths share one extent, matrix and exact-form decision. Identity is an exact no-op; exact quarters delegate bit-for-bit including PPI/chrome; filtered angles resample every raster, every mask and every saved channel into the expanded canvas. A 32-bit float document and retained sources refuse by name. |
| `flip_horizontal`, `flip_vertical` | None | Entire document. |

`target` is `pixels` or `mask`. Filters are `nearest`, `bilinear`, `bicubic`
(Catmull–Rom), and `lanczos3`. Arbitrary group-content and adjustment-content
transforms refuse. A mask attached to a group or adjustment is independently
transformable. No filter aliases, Gaussian substitution or byte fallback exists.

A leaf-content transform carries a linked mask through the same canvas mapping
in the mask's own storage extent. An unlinked mask stays fixed. Mask-only edits
retain the raster allocation and placement. Whole-document operations move all
masks regardless of their link flag, preserving density, feather and enabled
properties. Stored feather radii retain their existing pixel-unit semantics.
Groups, live adjustment descriptions, layer properties, profile identity,
metadata and channel identities survive geometry.

Resize preserves the established size/placement policy: multiply each stored
extent and offset by the canvas scale, round dimensions and offsets separately,
and retain a minimum one-pixel storage dimension. It resamples whole independent
planes, including off-canvas data. Crop rebases raster and mask offsets without
trimming their raw storage, while canvas channels receive exact subrectangle
copies. Overflowing destination coordinates refuse instead of saturating.

## Sampling contract

[The shared spatial walks](../core/src/sampling_walk.rs) own coordinates, tap
order and edge behavior. [Sampling](../core/src/sampling.rs) owns kernel formulas,
premultiplication and straight-alpha conversion. The established byte path keeps
its f32 arithmetic and opaque image-crate resize fast path.

Exact flips, quarter turns, integer translations and nearest sampling preserve
raw u16 codes, including hidden RGB at alpha zero. Integer translations share
the original pixel/coverage allocation. Typed nearest resize uses exact rational
pixel-center index selection to avoid f32 index rounding on long planes.

Filtered sampling uses premultiplied f32 accumulation, with no intermediate
integer quantization. The final straight color divides by the unrounded alpha
before clamping and rounding to u16. Hidden RGB contributes no neighboring color;
an output whose alpha rounds to zero has zero RGB. Affine and perspective use
transparent out-of-bounds taps. Resize clips and normalizes its source support,
widening that support on reduction to suppress aliasing.

Typed transform weights evaluate the **same** kernel polynomial/window formulas
in f64, normalize in f64, then enter the f32 accumulator. This addresses
Catmull–Rom polynomial cancellation near its support edge: the initial f32
weight path exceeded the existing numerical bound on a low-coverage affine
edge. Byte weights are unchanged. This specialization neither duplicates the
kernel equations nor widens the acceptance threshold. Resize retains its shared
f32 weight and intermediate policy.

## Admission, purity and cancellation

[The geometry plan](../core/src/precision_geometry_plan.rs) contains final canvas
dimensions, retained bytes, scratch bytes and work units. It validates every
raster/mask/channel destination and hypothetical native reopen before allocating
an output. Scratch includes the bounded request/tree bookkeeping, all new output
planes and the largest simultaneous resize intermediate/weight allocation. Work
includes initialization, tap visits and final no-op comparisons. Existing
100-MP plane, native aggregate, 2-billion-work-unit and 2-GiB working limits
remain applicable; admission includes caller-retained state.

Operations capture immutable handles and publish a complete new document or
return an error. Semantic no-ops return no replacement. Shared sample loops
check cancellation within rows and every 4096 samples/taps. Typed progress is
scoped to the operation; legacy intermediate callers retain their former
checkpoint-only behavior. The same token remains owned by the outer FFI call
through request parsing, planning and result publication, and every exit clears
its scratch reservation.

## Qualification

[Independent geometry tests](../core/tests/precision_geometry.rs) cover all
65,536 source codes on a nonsquare image, exact alpha/hidden-RGB permutations,
u16 masks/channels and native reopen; linked/unlinked placement; group and live
adjustment masks; whole-tree resize/crop; strict requests, admission and active
cancellation. Filtered affine, a separately solved projective trapezoid and
anti-aliased reduction are compared with direct f64 reference equations.

The filtered storage gate remains **2e-6 + 0.5/65535** in normalized units;
exact paths require exact codes. Numerical maxima and final test/build evidence
are recorded with the batch result in the high-depth status.

The focused debug run passed all eight tests. Its maximum stored affine errors
were 7.6060e-6 for bilinear, 7.6848e-6 for bicubic and 7.6265e-6 for Lanczos3,
below the unchanged 9.6295e-6 gate. Cancellation requested after filtered
sampling began returned in 2.421 ms on the 1600 × 1000 fixture, releasing the
reservation and retaining the source. That measurement describes this fixture
and machine; it is not a universal scheduling guarantee. The run log is
`/tmp/rz-phase7d-geometry-tests.log`.
