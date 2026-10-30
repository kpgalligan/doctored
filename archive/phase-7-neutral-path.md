# Phase 7B: preserving neutral workflow

This describes the completed 7B baseline. [7C](phase-7-tone-color.md) extends its
typed workflow with shared tone/color processing and native v12 adjustment
records while retaining v11 reads. The restrictions below describe 7B's scope.

This slice implements the u16 storage decision from
[7A](phase-7-high-depth-decision.md). It is an internal workflow with distinct
Rust and Swift types. Public document mode selection, conversion commands and
editor Undo/recovery integration remain in 7E. See the
[status](phase-7-high-depth-status.md) for qualification results and the next
assignment.

## Storage and evaluation

`PrecisionDocument` owns immutable u16 raster sources and a bounded tree of
isolated Normal and pass-through groups. Its explicit policy is `rgba16_unorm` storage and
`encoded_rgb_f32_v1` evaluation. Layer names, placement, visibility and opacity,
one captured RGB profile, print resolution and the existing EXIF/XMP/IPTC
packets accompany the sources. Neutral duplicate, opacity and grouping edits
return new handles; unchanged opacity creates no edit.

Rendering translates the supported tree into the shared precision renderer.
Group accumulators remain f32, and only the authoritative u16 projection rounds
to storage. Source latent RGB and alpha remain unchanged. Rendered transparency
follows the existing compositor's zero-contribution policy. The 7A alpha floor,
record/depth/work limits and 2 GiB tracked working-memory ceiling still apply.
The tracked ceiling is not an RSS ceiling.

Masks, saved channels, other blend modes, adjustments,
styles, geometry, painting, retouching and profile conversion are unavailable
in this document type. Unknown persisted feature/evaluation tags refuse rather
than degrade. No alternative adjustment or color-conversion suite is added.

## Import and delivery

The preserving importer accepts genuine unsigned RGB/RGBA16 PNG and classic
TIFF. It preserves individual sample codes, applies orientation as a sample
permutation, and transposes resolution axes when required. Untagged RGB is
explicitly interpreted as sRGB and disclosed in import warnings. Unsupported
depth, profile, alpha or container semantics produce errors. This path has no
ImageIO fallback and does not automatically adopt the editor working space.

Pass-through groups evaluate children against the enclosing backdrop and gate
their combined contribution with the existing premultiplied opacity kernel.
That kernel is shared with the byte document renderer, with its old arithmetic
and exact zero/one endpoints preserved.

PNG16/TIFF16 export renders the captured typed document, then uses the shared
metadata sidecar and TIFF layout code. The existing per-packet preservation,
normalization and carried-packet reporting apply. Destination publication is atomic after
successful encoding. Neither native saving nor delivery accepts an RGBA8
preview as an input.

The bounded PNG writer currently uses uncompressed pixel data, including native
layer PNG payloads. Files can therefore be large, and separately serialized
duplicate layers occupy separate disk space even when their in-memory sources
share one allocation. Compression tuning and regional/tiled backing remain
outside this slice.

The C header keeps the existing `RzDocument` and `RzImage` byte contracts
unchanged. New `RzPrecisionDocument` / `RzPrecisionImage` owners expose exact
native-endian u16 sample reads and an explicitly named byte-preview conversion.
The Swift wrapper retains source ICC separately from its byte display image.
No high-depth handle can enter an existing byte editing API through these types.

## Native state and host captures

Native v11 declares storage and evaluation before its typed layer records.
The reader validates the complete record structure and serialized capacity
before decoding pixels. Writer and reader share the same limits. Source
planes are lossless RGBA16 PNG payloads; document metadata stays separate from
the layer pixel payloads. Legacy documents continue writing v10 and retain
their existing group quantization and adjustment semantics. The byte reader
refuses v11 before decoding; native failures also stop before the app's
generic ImageIO fallback.

Internal Swift captured saves retain one exact typed source while subsequent
immutable edits can proceed independently. A versioned neutral host archive
stores that native source plus bounded plain manifest values. It validates
storage/evaluation and dimensions and uses the typed reader exclusively.
It is separate from the public `.rzsnapshot` and recovery packages: it proves
the preserving capture/serialization seam, without creating another Undo
manager or exposing an incomplete editing mode.

The weak host registry inventories all live typed documents and images in one
deduplicated union, including captures and projections. Existing byte and typed
unions are added conservatively; shared profiles across those families can be
counted twice. Codec, projection and native plans include sample width and
serialized repeated planes rather than assuming four bytes per pixel.

## Execution boundaries

Composite and explicit preview loops poll the single-use core job control.
PNG/TIFF dependency calls cannot poll within a frame, so codec cancellation is
checked at stages and during owned conversion loops. The compound export is
truthfully noncancellable. Internal host saves use the existing bounded worker;
the source owner remains alive until completion. Failures keep prior handles
and destinations intact.

Qualification passes for independent sample references, typed FFI refusals,
legacy appearance, metadata/orientation, host capture/archive round trips and
the declared 24 MP workflow. The [status](phase-7-high-depth-status.md) records
the 911-test suite, app/probe checks and exact memory/measurement scope. The
24 MP editing layers share one source allocation, and process RSS exceeds the
tracked working limit. A resource refusal is a failed size/workflow target;
no smaller substitute or raised limit counts as a pass.
