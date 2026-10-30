# Phase 7C: shared tone and color processing

The internal 7C workflow is implemented and qualified. Results and the 7D handoff are
recorded in [the phase 7 status](phase-7-high-depth-status.md). Public high-depth
editing remains gated on 7E; the Phase 6 cutout quality gate is unchanged.

## Evaluation contract

Typed adjustment descriptions use the existing `type`, `op` and `params`
envelope with required top-level `evaluation: "precision_v1"`. They use the
existing adjustment interpreter and algorithms. Unknown operations, targets or
evaluation tags refuse. Authored Curves coordinates remain on the established
0–255 parameter scale; evaluating a curve no longer rounds the input or output
to a byte. Explicit posterization and thresholding remain intentional effects.

Live adjustment layers evaluate the enclosing float backdrop and retain f32
values across the stack and supported groups. Destructive application operates
on a captured raster source, rounds its final RGB once to u16, preserves alpha
exactly and leaves zero-alpha latent RGB unchanged, following the established
adjustment behavior. Repeated destructive writes still incur u16 storage rounding.

Exposure and White Balance default to the existing calibrated `document_v2`
parameter policy in typed descriptions. The shared profile pipeline now has a
continuous encoded output. Explicit `legacy_srgb` retains the existing numeric
sRGB interpretation; it is separate from the required typed evaluation tag.
Shadows/Highlights uses float luma and a shared Gaussian guide kernel. Gradient
Map's optional dither becomes ±half a u16 code. Authored hex colors retain their
byte-valued parameter schema, and gradient positions/opacity retain the existing
four-decimal normalization. Stored LUT grids and Float32 entries retain the
existing import limits; these parameter policies do not quantize source pixels.

## Color context

Assignment changes the captured profile and shares the exact source samples.
Conversion transforms every distinct stored RGB allocation, including hidden
RGB, preserves alpha and source sharing, and clamps destination gamut in the
bounded SDR policy. It does not perform automatic working-space adoption.

Live adjustment parameters remain unchanged across assignment/conversion and
are prepared in the new captured profile, matching the established document
color operation. A nonlinear live stack can therefore change its composite
appearance after source conversion. This operation does not flatten or bake the
live stack to promise appearance preservation.

Exact ICC bytes are a no-op. Exact numeric matrix/TRC identity bypasses the
encoding round trip. The legacy approximate-equivalence rule cannot skip a
high-depth conversion. Source and destination transform profiles must satisfy
the supported monotone matrix/TRC contract; retained unconvertible RGB profiles
remain available for operations that do not require a calibrated context.

The continuous ICC encoder uses the shared forward curve evaluator and a
65,537-position encoded grid. Interpolation has a separate worst-case encoded
uncertainty below `1/65536`, before f32 arithmetic and final u16 rounding. This
is not a change to the existing `2e-6` compositor bound. Tests distinguish exact
source preservation, float arithmetic, ICC interpolation and storage rounding.

For a non-strict tone curve, the encoder chooses the lowest preimage, including
terminal plateaus; converting through a noninvertible curve does not promise a
source-code round trip. Source and destination curves are validated separately.
Malformed decreasing curves, undefined/nonfinite parametric powers, overflowing
spline coefficients and unsafe LUT values/domain spans refuse in the typed path.

## Persistence and execution

Typed native v12 adds explicitly versioned adjustment records. The reader
retains v11 neutral reads; byte documents keep their v1–10 interpretation and
v10 writer. Complete native record and capacity preflight precedes pixel decode.
Captured typed edits/saves retain their source owner, and typed host archives
retain backward reads of the v1 neutral manifest. Public central Undo, snapshots
and automatic recovery remain the 7E integration task.

Preparation and pixel work use the fixed 2 GiB tracked-memory policy with
caller-retained state. Plans include ICC tables, parsed lookup tables, guide
planes and output buffers. Shared source allocations are deduplicated, while
serialized raster records still count separately. The tracked limit remains
an admission estimate, not a process RSS ceiling. Unsupported masks, scalar
planes, geometry, paint, retouch, clipping, styles and other blend modes remain
unavailable in the typed workflow.

The JSON preparation allowance is `64 × description bytes + 4 × ICC bytes +
4 MiB`, covering parser growth, canonical text, profile copies and encoding
tables. Native reopen conservatively charges each serialized operator's full
preparation allowance alongside the planes, even when live duplicates share one
prepared owner. Long simple stacks can therefore refuse before the nominal
1,024-record cap. Separating maximum parser scratch from summed retained
operator data is a possible later optimization; the working limit is unchanged.

ICC/JSON preparation checks cancellation at stage/operator boundaries. Dense
raster and float-guide loops poll cooperatively. Existing codec stage boundaries,
noncancellable compound export and uncompressed PNG/native raster payloads remain.
Compact descriptor queries avoid copying layer names and large LUT descriptions
while planning captured work; explicit detailed inspection remains separate.
