# Phase 8E — retained artwork at native depth

Status: **complete within the qualified scope below**, following commit `943dbf5`
(Phase 8D). **P7-04 and P7-05 are verified.** Five Phase 7 assignments are now
verified and 31 remain open. Continue with 8F/P7-03; 8G remains required before
Phase 8 as a whole completes. Progress and qualification are saved in this file.

## Retained model and editing

Both document depths retain editable text and shape descriptions beside their
authoritative raster. Native/recovery reopen displays the saved raster without
font lookup or rerendering. Description v3 stores continuous normalized sRGB RGBA
and up to 64 ordered, nonoverlapping attributed text color runs, indexed by whole
composed-character UTF-16 ranges. Legacy v1/v2 descriptions remain readable;
v3 requires its new artwork schema and older schemas cannot acquire it silently.
New authored colors outside normalized sRGB refuse instead of falling back to a
rounded hex value. Legacy hex values remain the source limit of legacy content.

The existing text/shape tools, double-click editing, numeric transforms, whole
image resize/rotate/flip and Assign/Convert Profile regenerate from the retained
parameters. Numeric Artwork Colors supports default ink, character ranges and
shape fill/stroke; line shapes expose stroke only. Typography edits preserve
attributed colors. Actions retain full numeric colors, UTF-16 runs and affine
values. Insertion lands at the slot the `position` / `layer` pair names at BOTH
depths since [12E-2b](phase-12e-layer-ops-status.md) — above the selected
subtree by default, which is what byte insertion always did and what the typed
path does now instead of appending a top root — and selects that actual new
layer. Artwork generation does not use
the canvas selection as a clipping mask; independent layer masks remain intact.

Assign Profile reinterprets ordinary raster samples but regenerates artwork from
its authored sRGB colors into the newly assigned profile. Convert Profile also
regenerates artwork in the destination profile. Rendering uses the existing
qualified matrix/TRC ICC transform and f32 compositor, with one final native u16
storage rounding; byte documents deliberately quantize that result once. No byte
preview becomes the source of a native rerender.

Layer > Rasterize Artwork and MCP `rasterize_artwork` drop only the description,
retain exact current pixels/profile/mask, and Undo restores editability. Typed
retained artwork supports affine geometry; true perspective requires explicit
rasterization. The byte editor retains its established true-perspective
rasterization behavior. Group/linked-set/deform and expanded interactive geometry
remain P7-28/12F; this slice does not broaden that existing geometry scope.

## Generated geometry and intrinsic assets

TextKit still owns layout and shaping. Capture uses each actual resolved glyph
font, position and text matrix, including emoji/CJK/RTL fallback fonts. Ordinary
glyph outlines and shape paths go to Rust. Underline/strikethrough paths come from
the superclass decoration callback through a bounded PDF path capture, preserving
platform placement instead of guessing metrics.

Rust flattens curves to a 1/4096-pixel point-to-segment bound, including collinear
backtracking and cusps. It integrates linear-edge area between row endpoints,
integer-column crossings and edge intersections; fractional x and y translations
therefore preserve native coverage distinctions. Nonzero and even-odd winding
are supported. This is geometric coverage, not a claim of pixel identity with
TextKit's hinted glyph rasterizer. Independent straight-edge references allow
one u16 code; the separately converged curve corpus allows 32 codes.

Intrinsic color glyphs use the original superclass drawing callback in an RGBA8
sRGB context. Their original byte-valued color/alpha is disclosed, while authored
opacity stays continuous and separate. Visible monochrome bitmap glyphs use A8
intrinsic coverage with separate continuous authored ink. These assets do not
restrict native generated outlines or other colors to 256 levels. The on-canvas
editor applies intrinsic opacity consistently with the committed renderer.
Installed-font qualification exercises color emoji; the visible monochrome bitmap
fallback is bounded and implemented but is not represented by a standard installed
font in the current host corpus. Core composition independently qualifies its
scalar/continuous-color contract.

Font files are not embedded. A saved rendering stays usable when fonts are
unavailable; a new rerender requiring an unavailable base font refuses. The
qualified outline result follows the saved layout parameters and installed font
implementation, not a promise of identical outlines across OS/font versions.

## Persistence, admission and cancellation

Typed native v20 adds a bounded `ADSC` table to the existing postorder document
records. Its explicit source-table sub-version retains mixed RAW, linked and
ordinary assets. References, description framing/schema/UTF-8, total header and
pixel sizes preflight before any PNG decoding. A raster cannot simultaneously
carry an artwork description and a retained-image binding. Older compatible
writers remain in use when descriptions are absent. Byte descriptions use the
existing opaque layer metadata slot. Unknown future descriptions preserve their
saved pixels in a host that supports the native container. They refuse semantic
rerenders and retain cached pixels until explicit rasterization; older native
readers that do not support v20 refuse the container.

Raw core pixel/geometry/Assign paths invalidate stale descriptions; the host
atomically supplies corrected descriptions and regenerated pixels. Typed presence
uses cached descriptor lengths, so an admitted full-read refusal cannot classify
artwork as plain raster. Crop/affine metadata restoration fails closed unless the
expected description is already present; a failed restore never publishes a
raster-only fallback. Pure content
replacement preserves independent masks and layer properties. Shared descriptor
allocations participate in resource accounting and native save capacity.

Platform layout/capture is admitted, bounded and noncancellable on the main
thread. Value-only arrays and leased payloads enter the existing single worker;
Rust flattening, coverage, composition and publication cooperate with cancellation.
A latched parent token hands cancellation to each fresh single-use child token,
with monotonic weighted progress and active-child scratch reporting. Cancellation
between stages cannot be lost. Final publication rechecks source identity,
revision, target and selection in both depths, including modal color-sheet state
captured when the sheet opened.

Limits include 4,096 UTF-16 text units, 64 color runs, 256 ordered components,
64 million combined component samples, 262,144 path/flattened-edge elements, a bounded
256 million scan-work count, and 64 KiB descriptions. Coverage reserves a conservative
32 MiB plus output/row storage before flattening, including vector growth and
intermediate overlap. Platform preparation and payloads have separate admission.
These are refusals beyond a bounded implementation, not an OS RSS ceiling or an
arbitrary large-document guarantee; larger text/canvases remain P7-34/13D and
regional processing remains 13A–13C.

## Qualification

- Independent Rust coverage/composition/FFI: **18 passed**. Exact fractional x/y
  rectangles, independently clipped slants, 1,024 distinct coverage values,
  winding/holes/intersections, quadratic/cubic integrals and converged cusp/
  backtracking references; continuous ink/intrinsic opacity; caps, admission,
  cancellation, atomic outputs, no-ops and all six NULL export paths.
- Independent native/model tests: **8 passed**. Mixed v20 RAW+linked+artwork,
  duplicate nested groups, exact profile/codes/masks/channels, byte v18 parity,
  opaque future descriptions, sharing/no-ops, invalidation and source-intake
  refusal. Poisoned-PNG canaries establish malformed description preflight before
  decoding. Focused log: `/tmp/rz-phase8e-independent-core.log`.
- Full Rust source regression: **1,102 passed, 0 failed, 2 ignored**, across 109
  result groups. Format and all-target Clippy with `-D warnings` pass. All 314
  measured build inputs and the file set remain unchanged. Report:
  `/tmp/rz-phase8e-core-summary.json`; log `/tmp/rz-phase8e-core-tests.log`;
  input manifest `/tmp/rz-phase8e-core-provenance.json`.
- Final actual app-source host snapshot: **37 passed** (19 capture/editor/
  admission and 18 actual controller/UI/cancellation groups). Four intrinsic-alpha
  editing/commit comparisons are pixel exact. Outline layout stays within the
  declared 2-pixel neighborhood and 2% alpha-area gate against hinted TextKit;
  actual maximum area drift is **0.71512%**, not pixel equality. Decorations have
  exact bounds and at most one alpha code error; transformed decorations are
  exact. Numeric color/range UI, selected-color preservation, shape resizing,
  stale target/selection, below-top typed insertion/Undo, controller Undo/Redo
  and between-stage/in-flight child cancellation/scratch all pass. Final report:
  `/tmp/rz-phase8e-text-workflow-host-final5.json`. All 329 tracked / 327 compiled
  inputs and the file set are unchanged; owned PID 11015 exited. Host binary:
  `76fd32743eda34ae1f06d07aee373c12664b306dcfc45e543f35faa3c7cbe229`.
- Corrected Release-app external MCP: **26 groups passed** (12 byte, 14 typed),
  with 162 catalogued tools, 645 exact input hashes/file set and all four owned
  processes exited. It checks precise attributed/shape fields and recorded
  Actions, below-top typed insertion, no-ops/Unicode refusals, affine down/up,
  whole-image geometry, Assign/Convert, masks/opacity, explicit rasterization,
  native reopen/edit and actual controlled crash-recovery/edit in both depths.
  Transparent native ellipse output has **1,986 alpha values**, 1,975 not on the
  byte grid; ordinary text has **4,100**, 4,076 not on that grid. Each producer's
  **1,179,648 components** survive native reopen exactly. Report:
  `/tmp/rz-phase8e-mcp-final.json`. The tested app hash is
  `19c4a25f29e1d99a8c914382fa2161238823dd70fee2cfcced15938f60eb864a`.
  An earlier typed inspection failure was fixed by merging shared authored-field
  builders into the dedicated typed `get_document` dispatch under aggregate
  parse/output admission; the strict equality gate was retained.
- A deterministic 64×64 host regression uses a synthetic accounting lease that
  leaves one byte of headroom, without allocating giant pixels. It proves full
  metadata-read denial while cached presence/policy remains true, crop/affine/
  profile/restoration refusals, identity on unchanged restoration, and unchanged
  original pixels/metadata/dirty state after release. This closes the final audit's
  failure-path gap; no raster-only fallback publishes when restoration fails.
- Authoritative Swift typecheck passes with zero errors:
  `/tmp/rz-phase8e-typecheck-final5.log`. Release build passes:
  `/tmp/rz-phase8e-app-final.log`. The unchanged 26-group external MCP and 37-group
  host suites both ran against the final integrity-fixed source. Earlier reports
  remain historical snapshots and are not substituted for the final results.

Builds use `MACOSX_DEPLOYMENT_TARGET=15.0`. The release archive was built before
full test linking so app compilation could proceed without waiting on Cargo.
Its SHA-256 is
`757cd8620f761785ef4d19fb3def3ab328b17c974e581568d574b44b02e5eafb`
(97,796,792 bytes). Full Cargo tests qualify the recorded release source graph;
they are not described as executing that exact static archive. Actual-source
host probes and the final app separately qualify the copied/linked archive and
record their own hashes.

Reproduction uses the checked-in independent probes:

```sh
MACOSX_DEPLOYMENT_TARGET=15.0 make typecheck
MACOSX_DEPLOYMENT_TARGET=15.0 make app
MACOSX_DEPLOYMENT_TARGET=15.0 make lint
MACOSX_DEPLOYMENT_TARGET=15.0 make test
python3 scripts/verify_phase8e.py --app build/Build/Products/Release/Rasterize.app/Contents/MacOS/Rasterize
python3 scripts/artwork-text-probe/run_host.py --workflow-ui --require-current --report /tmp/rz-phase8e-text-workflow-host.json
```

Run `cargo fmt --check` in `core/` as well. Test runners snapshot/hash inputs,
use dummy credentials and private Actions/recovery/RAW-preset stores, select
nondefault ports and terminate only their own processes. Graphics/font-service
access is required for real AppKit qualification; an access-denied empty rendering
must fail instead of becoming a reference.

## Handoff

8F still owns P7-03 host/frame precision and retained-source behavior. 8G still
owns local RAW masks and additional qualified camera/lens controls. Phase 8 as a
whole remains open until both finish. Brush/selection byte coverage is separate
from generated artwork; P7-06/11/13 and their receiving slices remain open. No
Phase 6 cutout criterion or historical Phase 7/8 measurement changes here.
