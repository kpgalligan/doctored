# Phase 4 rendering and kernel contract

Phase 4 introduces a bounded **pixel-kernel** request and routes the existing
destructive Invert through it. The document compositor still renders full frames.
This distinction keeps the delivered behavior measurable: a 512 × 512 request
reads and writes 262,144 pixels regardless of the source image dimensions.

## Pixels and evaluation

`RzImage` remains tightly packed, top-row-first **straight RGBA8**. Its handle carries
no ICC profile; the owning document or explicit host context identifies the RGB
space. Copy and Invert operate on those encoded channel bytes, make no transfer
curve or matrix conversion, and preserve alpha exactly. Invert is `255 - channel`,
including latent RGB under zero alpha. The shared byte kernel lives in `ops.rs`;
the float adjustment-layer twin retains its existing `1 - channel` semantics.

The plan reports `rgba8_unorm` storage, `exact_u8` processing and
`encoded_rgb_passthrough`. It does not imply linear-light processing or a change
to saved document rendering. Profile-aware Exposure and White Balance retain
Phase 3's separately versioned `document_v2` evaluation. No native version change
is needed for this request, which adds no persisted state.

Pointwise kernels read one source sample and preserve its alpha. Future spatial
kernels must use the existing `sampling.rs` premultiplied floating-point sampling
and unpremultiply at the storage boundary; they must not interpolate hidden
straight RGB. Scalar masks remain scalar coverage. A preview may use lower spatial
resolution, but it must never replace the authoritative source. Higher-depth
storage requires new typed interfaces and explicit conversions in Phase 7.

## Requests and requirements

`rz_image_render_plan` and `rz_image_render_region` share one validated
`RenderRequest`. The request names Copy or Invert, a nonempty source-coordinate
rectangle, a positive integer preview divisor and an output-byte budget.

| Property | Contract |
| --- | --- |
| Region | Fully inside the immutable source; overflow and out-of-range rectangles are errors. Output starts at the requested source origin. |
| Divisor 1 | Exact requested pixels; output dimensions match the rectangle. |
| Divisor greater than 1 | Preview only; dimensions use ceiling division. Sample `(x + i × divisor, y + j × divisor)`. No interpolation or implicit centered shift. |
| Source requirement | Copy/Invert are pointwise with zero halo. The declared input rectangle bounds all samples; the exact read count is the output-pixel count. |
| Output | One owned tightly packed image, freed by the caller. Region origin and the preview flag stay in the request/plan because `RzImage` contains only pixels. |
| Allocation | One output allocation of `output.width × output.height × 4` bytes and no scratch heap allocation. The caller's retained input is separate. Refuse the output budget before allocation. |
| Persistence | Requests and previews are transient. Existing UI/Actions Invert still produces a full-sized authoritative result, so target/selection writeback and undo stay unchanged. |

The plan is owned JSON with `contract_version: 1`, pixel/profile/alpha policies,
requested and required input rectangles, neighborhood, preview policy, exact
output/read counts, and cancellation cadence. Planning performs no pixel scan or
output allocation. Unknown operation codes fail explicitly.

Spatial migration must declare a source-coordinate halo, border extension rule,
and conversion between source units and preview units **before** rendering. A
request near a border must use the same edge semantics as its full-frame
reference. Gaussian Blur, for example, cannot crop first and blur that crop as
though its edges were source-image edges. Its existing shared sampling kernel
remains the reference until a padded regional adapter is delivered.

An operator needing global analysis must explicitly declare full-source analysis,
its analysis scratch bound and whether evaluation can then be regional. Until
adapted, it may use a declared **full-frame fallback**, followed by requested-region
extraction, with a full-frame allocation estimate. The caller may reject that
fallback under its budget. Auto Contrast's histogram and the layered compositor
currently remain on their existing full-frame APIs; neither is accepted by the
new pointwise entry point or mislabeled as bounded. No automatic fallback hides
an unsupported request.

The future source/filter/transform/mask/style/composite evaluator must carry the
document's evaluation version and profile through each stage. The current stage
order is unchanged; this kernel request is not an alternative compositor and does
not invent a new persisted evaluation order.

## Cancellation, ownership and errors

A caller may supply the same single-use `RzJobControl` used by Content-Aware Fill.
The FFI claims the control after validating the plan, reserves output bytes against
its scratch budget before allocation, and keeps an RAII run guard until return.
The source handle and control must remain alive for the call. Immutable source reads may overlap across threads; the host retains each handle
until every reader finishes. Cancellation/progress queries may use the control
concurrently. Neither render function touches document caches or AppKit.

The renderer checks cancellation before output allocation, at most every **4096
output pixels**, and before returning a completed result. This is a computational
work bound, not a guarantee about OS scheduling or allocator latency. Progress
reports completed output work with stage 6 (rendering), then stage 5 (complete).
On success the new image transfers to the caller. On cancellation/error, partial
output is dropped; on every return the run guard releases its scratch reservation.

Invalid bounds, unsupported operations, budgets, cancellation and allocation
refusal return `NULL` plus an owned error string. A caught panic is an explicit
internal error. A null source is tolerated and returns null. No error returns a
partial image, substituted black pixels or a successful-looking no-op. Copy is a
render primitive, not an edit: callers must not register an undo step solely
because they requested a copy.

## Verification and measurements

`core/tests/regional_render.rs` derives byte values and source coordinates
independently. It checks full-image Invert, stitching uneven tiles, hidden RGB and
alpha preservation, nonzero origins, odd-size preview cells, oversized divisors,
source immutability, exact budget boundaries, overflow refusals, shared-token
cancellation/progress/single-use behavior and reservation cleanup. Both new FFI
exports are included in the null-safety sweep.

The five regional FFI tests pass. The AppKit jobs probe additionally passes the
actual Free Transform session's parameter controls/commit, responsive progress,
one-step undo/redo and Action replay; MCP-handler perspective and grouped
transforms agree with the established core reference; transformed descriptions
are dropped and restored by Undo; and mutation followed by Undo rejects a stale
worker result even when it returns to the identical source handle. The shared
worker reservation returns to zero after completion. `make typecheck` passes
with the six existing warnings.

`cargo run --release --example phase4_latency` measures full/512-pixel-region/
divisor-8 Invert, exact rotation, arbitrary bilinear transformation, native
save/open and PNG export with projection. The fixed synthetic corpus is shared
with resource measurements: 6000 × 4000 and 8000 × 6000, opaque RGBA8 with
`r=(17x+3y)%256`, `g=(5x+11y)%256`, `b=(x xor y)%256`. Each operation runs three
times serially, reports wall time as JSON, checks for failure and releases the
result. Temporary saved files are deleted by the runner. These are migration
measurements on one machine, not photographic-quality or production-scale claims.

### Phase 4D migration measurements

Measured September 9, 2026 on an Apple M2 Max with 96 GiB RAM, macOS 26.5.1
(25F80). Core timings use optimized Rust; RAW uses the same `-Onone` Swift mode
as the qualified integration app. Each entry is the median of three serial
runs; parentheses show the observed minimum–maximum. Saved files use a local
temporary directory and the synthetic pattern above, so codec and storage
timings are fixture-dependent.

| Existing path | 24 MP milliseconds | 48 MP milliseconds |
| --- | ---: | ---: |
| Exact document 90° rotation | 86.9 (85.7–87.7) | 147.8 (143.8–156.7) |
| Raster affine, 10° rotation × 90% scale, bilinear | 555.7 (551.8–556.9) | 1073.3 (1073.2–1093.1) |
| One-layer native save | 193.4 (127.7–382.2) | 468.6 (466.8–471.9) |
| Native open | 258.8 (257.3–270.2) | 512.5 (509.5–603.3) |
| PNG export including projection | 565.2 (359.3–679.3) | 1115.0 (798.8–1178.0) |
| Actual RAW development to RGBA8 | 1144.1 (1133.7–1165.5) | 2282.3 (2250.7–2380.5) |

The native files are approximately 49.4/98.9 MB. The RAW fixture is an
uncompressed 16-bit RGGB CFA DNG with a declared camera matrix and camera
orientation; it develops to 4000 × 6000 / 6000 × 8000. Its contents test decoder
work and transfer, not camera quality. `scripts/phase4_raw_latency.py` compiles
the actual RawImage, Bitmap and settings source, excluding unused document and
pasteboard wrapper extensions and the core TIFF density fallback. It runs the
real Core Image render, output-profile validation and unpremultiply pass. A
sandboxed Core Image run produced invalid zero pixels and was rejected; the
recorded measurements use the GPU-capable isolated probe. No user document,
preferences or credentials are read.

These measurements justify moving final raster affine/perspective transforms,
save/export and RAW development to the bounded worker seam. The new
`TransformWork` value captures the immutable document, matrices/quads, sampler and
indices whose descriptions must be dropped. UI and MCP commits share it; a lone
editable described source still composes and renders on the main thread. Worker
results include a prepared projection, then the main thread verifies both the
captured document handle and monotonic revision before one undoable edit.
Mutation followed by Undo still rejects the stale result.

Transform resampling, platform RAW development and encoders do not yet have
cooperative inner-kernel cancellation. Their progress panel is responsive and
indeterminate and offers no Cancel action. This is explicit; the cancellable
Content-Aware Fill/spot-heal path and the bounded regional kernel have different,
measured cancellation contracts. Exact whole-document geometric commands and
ordinary native open remain on their existing paths.
