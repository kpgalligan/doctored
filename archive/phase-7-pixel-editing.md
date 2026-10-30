# Phase 7D masks, paint and retouch contract

These are internal operations on the separate immutable `PrecisionDocument`.
The public editor still uses its established byte document. Qualification and
the 7E handoff are tracked in [the batch status](phase-7-geometry-paint-status.md).
[Geometry](phase-7-geometry.md) has its own sampling contract.

## Storage and masks

Raster sources remain straight RGBA16 UNORM in one captured document profile.
Masks and saved channels store native u16 scalar coverage. Source alpha is never
converted through a byte coverage API. Masks have independent dimensions and
canvas placement, linked/unlinked content geometry, enabled state, density and
feather. Channels are canvas-sized named planes with session identities.

The compositor prepares feathered coverage as f32 using the existing shared
Gaussian and multiresolution blur schedule. Raw coverage stays exact. Density
uses the shared formula; full density has an exact identity branch so tiny
coverage survives subtraction. There is no retained typed blur cache in this
batch: preparation is planned scratch and drops after each layer is evaluated.
Ancestor masks prepare after their children, so the renderer reserves the
largest mask workspace alongside its group buffers and adjustment guide.

Masks gate raster alpha, live adjustment strength, isolated group alpha, or the
combined pass-through contribution. Their coverage remains f32 until the final
projection store. Effective-mask inspection evaluates density and feather even
when the mask is disabled, matching the byte inspector. Rendering bypasses a
disabled mask. Raw-mask reads ignore all properties.

The original prototype's 2^-32 effective-alpha floor excluded legitimate
alpha-code-1 × mask-code-1 × partial-opacity products. The qualified floor is
now 2^-96. Multiplication preserves whether each input was actually zero:
positive underflow and positive results below the floor refuse. The ordinary
**2e-6** float arithmetic and **2e-6 + 0.5/65535** stored-output error gates do
not change. This extends normalized SDR coverage; it does not add HDR.

Native **v13** adds raw little-endian u16 masks/properties to each record and
named channels at the document tail. Typed v11 and v12 readers remain; the byte
writer stays v10 and refuses typed versions before pixel decode. Full borrowed
record/scalar framing and operator validation precede all PNG/scalar decoding.
Writer, reader and hypothetical edit plans count every serialized raster/mask/
channel plane even when live revisions share allocations. The common decoded
byte ceiling and tracked working limit remain unchanged.

## Painting and sources

Paint and fill use a canvas-sized u16 coverage plane plus a continuous straight
RGBA color. `color_space` is explicitly `document` or `srgb`; sRGB authored colors
use the same continuous ICC matrix/TRC conversion as 7C. No RGBA8 colored dab,
CGImage, clipboard bitmap or display preview enters an authoritative source.

Normal painting calls the existing shared float source-over compositor and
rounds once when writing the u16 source. Erasure changes only alpha through the
shared eraser rule and preserves latent RGB. Zero coverage/opacity and exact
unchanged storage return no edit. Raster leaf storage outside the canvas stays
unchanged; paint does not expand a layer's stored extent.

Clone samples a captured raster leaf at integer canvas offset `(dx,dy)` relative
to each destination pixel. Sampling ignores that source layer's visibility,
opacity and mask, and always reads the original source, including when source
and destination are the same leaf. Off-source and transparent source pixels do
not paint. Clone uses source alpha and continuous destination compositing;
full opaque copies preserve all four source codes exactly. Current composite,
Sample All, fractional clone sampling and source-exclusion controls are outside
this batch.

Raw mask/channel replacement and property edits use typed coverage directly.
Their paint/filter/morphology UI targets, general RGBA-plane writeback and
selection semantics are not inferred from the raster paint implementation.

## Rasterized text and shapes

The existing TextKit layout and shape path/drawing plans serve both byte
rendering and internal typed artwork. Main-thread AppKit draws alpha-only
coverage. Typed text uses one uniform authored color; shapes use separate fill
and stroke coverage in their existing drawing order. Rust combines one or two
coverage/color planes in f32 and writes a single new u16 raster layer.

The explicit host adapter multiplies scalar bytes by 257. It carries only
**256 coverage levels**, while authored RGB stays continuous. This adapter does
not create high-depth information from a colored byte image. Color glyphs are
monochrome silhouettes in this uniform-color route; attributed color runs and
intrinsic emoji RGB are unsupported. Text is bounded to 4,096 UTF-16 units.
Native output stores the raster, without an editable text/shape description or
an automatic platform rerender path.

Drawing geometry is prepared on main with allocation admission before contexts
are created. The worker captures only values and scalar buffers. Platform text
layout/drawing allocations use a conservative estimate, not an OS memory bound.

## Healing

Healing uses an explicit canvas-space solve rectangle and an authoritative
captured raster source with integer offset. Coverage at or above normalized
one half enters the shared membrane domain. Source alpha must be positive;
destination alpha determines usable pixels and remains exact. Coverage outside
the solve rectangle has no effect. Within the contour, opacity controls RGB
writeback; fractional brush coverage does not additionally fade the solve.

Byte and u16 healing share one writeback implementation, region classifier and
Poisson multigrid solver. Their storage adapters select numeric units, contour
threshold, final quantization and the solver accuracy policy. The typed policy
adds at most 32 refinement cycles and checks an absolute residual of 1/16 u16
code. Failure discards the edit. A residual is not a general solution-error
bound: independent f64 harmonic references separately qualify tested domains
within two stored codes. The byte stopping schedule and rounding stay intact.

Admission includes the full destination copy, borrowed canvas coverage,
160 bytes per solve-window pixel for arrays/workspaces, and a conservative
three-channel sweep-work bound. The same shared solve-window ceiling applies;
the aggregate work ceiling can refuse a smaller rectangle first. There is no
implicit reduced-resolution solve. Buffer construction, source sampling,
classification, multigrid and writeback poll cancellation. Content-aware
synthesis/PatchMatch, spot healing and red-eye correction remain unavailable.

## C and Swift boundaries

Three exports share a strict UTF-8 JSON request of at most 64 KiB:

| Export | Contract |
| --- | --- |
| `rz_precision_doc_edit_plan` | Admitted retained/scratch plan and exact input sample count/layout; reads also report output count/dimensions. |
| `rz_precision_doc_edit` | Pure typed edit with optional borrowed u16 samples; exact no-op returns NULL without an error. |
| `rz_precision_doc_read_plane` | Exact raw u16 or once-rounded effective coverage, staged before publishing to the caller's buffer. |

Request families are `set_mask`, `remove_mask`, `mask_properties`, `set_channel`,
`remove_channel`; the geometry operations; `paint`, `erase`, `clone`, `heal`,
`rasterize`; and `read_plane`. Read targets are `mask`, `effective_mask`,
`channel`, `red`, `green`, `blue`, `alpha`. Operation-specific parsers reject
unknown fields/targets; unsupported operations do not fall back to byte calls.
Counts are recomputed from validated core dimensions before borrowing buffers.

The FFI owns the single-use job token from pre-parse admission through final
publication. Read output remains unchanged on failure/cancellation; its final
copy is a noncancellable publication stage. Plans count borrowed inputs,
caller output, staging and processing buffers. The bounded request parse has
stage cancellation and a 4 MiB reservation before allocation.

Swift calls remain exclusively in `RasterCore.swift`. Scalar buffers have
explicit dimensions, plane count, provenance and shared retained-payload leases.
Captures validate the full input layout, retain their source, and use the
existing bounded worker/resource registry. Independent captures conservatively
count separately; copies share their leases. Native saves and host archives use
the existing captured-source workflow. This adds no second Undo system.

7E must integrate these owners with the existing `ImageDocument` mutation,
history, recovery, selection/target validation, display, capability reporting,
MCP and Actions surfaces. Public exposure must retain the supported target
matrix and offer deliberate conversion for chosen unsupported operations.
