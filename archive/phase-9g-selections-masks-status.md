# Phase 9G — native selections, masks and planes

Status: **complete within the qualified scope below**, verifying P7-08–P7-11.
Subsequent [Phase 9H](phase-9h-paint-fill-status.md) qualification brings the
current Phase 7 ledger to 18 verified assignments and 18 open. The whole Phase 9
and the original P7-12/9I cutout quality gate remain open.

## Storage and transfer contracts

`CanvasSelection.Shape.mask16` owns canvas-sized u16 coverage. Positive native
coverage determines bounds, including values that round to zero in the display.
The grayscale image and contour are disposable byte display derivatives.
`maskSamples16()` returns native samples unchanged, or explicitly promotes a
legacy byte selection by multiplying by 257. The old `maskBytes()` transfer keeps
its byte ABI meaning. Existing geometric and platform analysis inputs retain
their actual source precision.

Selection algebra preserves the existing maximum/add, minimum/intersect and
`min(a, maximum-b)` subtraction meanings. Native invert, feather, smooth,
grow/shrink/border, affine transforms and Quick Mask use the additive bounded
u16 selection API. Native coverage survives selected tone/filters/Auto analysis,
retouch clipping, Patch translation, Content-Aware Fill, adjustment-layer masks,
saved channels and mask creation. Byte-only display clipping never replaces the
native write mask.

Quick Mask accepts either the historical six-byte run encoding or the new
eight-byte encoding: little-endian u32 run length, then premultiplied u16 gray and
u16 alpha. Runs must cover the canvas exactly and gray cannot exceed alpha.
Historical byte runs explicitly promote when applied to native coverage; native
source-over rounds once, preserving unchanged tiny values outside the stroke.
The existing tool records the coverage it actually generated, including its
legacy byte precision; Phase 9H owns the generated native brush/pressure engine.
`get_selection` keeps the original `coverage` field at 0–255 and adds exact
`coverage16`, the coverage maximum and authoritative storage name.

Editing-state packages use version **3**, with explicit readers for versions
**1 and 2**. Native selection files contain exactly two little-endian bytes per
canvas sample; current and previous selections retain their own type. Geometry,
file naming, extent, empty coverage and version checks precede materialization.
Selection/Quick Mask edits remain file-clean and use central history, snapshots
and recovery. Version 3 also stores optional channel-view state (RGB/plane/mask
and visible channel indices), validated against the decoded document. Target and
view restore together; old packages default to the composite view. Ordinary `.rz` documents continue to store reusable coverage in
masks/channels; transient selections belong to the editing-state package.

## Scalar edits and inspection

Native masks, saved channels and red/green/blue/alpha planes share one captured
scalar request. Whole-storage neighborhoods read the indicated original plane;
canvas selection independently controls writeback. Fill, brush/erase, the
existing destructive tone/spatial filter catalog and scalar morphology change
only that plane. Raw offsets, mask density/feather/link/enabled properties, other
RGBA components and zero-write-coverage samples remain exact. Scalar colors use
the shared core Rec. 709 gray conversion. Channel CRUD and options use immutable
native edits and central Undo. Scalar Auto Tone/Contrast/Color derives native
levels with whole-target or weighted-selection analysis, then applies those
reported levels without reanalysis. Apply Layer Mask uses its effective coverage
even when disabled, preserves raw RGB and multiplies only native alpha before
removing the mask in one edit.

Apply Image and Calculations use the shared blend table for native scalar planes
and RGB triples, including the existing HSL restrictions for scalar output.
Native Apply Image writes through the captured selection and preserves other
planes and raw storage outside the canvas. Calculations creates exact native
selection coverage or a saved channel. All nine luminosity masks share the
existing ladder formulas, preflight complete channel capacity and publish one
final document. These paths retain captured sources for previews, Actions and
central Undo. Their compound reservation uses the largest actual core source,
plane-read, blend or raw writeback plan plus simultaneously live native buffers
and display work. Input flattening and selected concatenation are admitted before
allocation; an undersized child token refuses before changing the document.

Channels and plane isolation use explicitly named display derivatives. Raw mask
and effective mask inspection are separate; effective inspection applies density,
feather and placement while ignoring the enabled switch. Raw layer/group inspection uses
captured native canvas-coordinate sources, and display reduction is never an
accepted scalar editing input. Selected tone sheets capture their scalar target
and selection for preview and final publication. Their histograms read complete
raw scalar storage with canvas selection weights mapped through its offset.
The additive scalar-statistics request shares the existing exact counter kernel;
plot stride never reduces endpoint counts or code sums. MCP `histogram` with
`source: "scalar"` names the same mask/channel/RGBA target explicitly.

`modify_scalar` mirrors Channels > Modify Plane. Its native morphology controls
are invert, feather, smooth, grow, shrink and border. Existing byte selection
morphology and byte scalar editing keep their original ABI and operation meanings.

## Source-derived selection

Alpha, luminance and individual colors are read directly from captured native
layer/composite/backdrop sources. Raw-plane placement uses the core's explicit
canvas read; channel-to-selection loading never passes through display bytes.
Magic Wand, Color Range and Quick Selection share the existing algorithms with
native source evaluation and u16 output. Lab analysis uses the captured document
profile. Native Color Range clicks sample actual u16 codes; MCP adds `samples16`
while preserving historical byte sample arguments. Wand keeps its historical
0–255 tolerance units and accepts fractional values for native thresholds.

Subject/object/people tools explicitly use a byte display derivative as Vision
analysis input and retain their declared u8 model coverage. They publish selection
only; native RGB/alpha remains untouched. Captured document, revision, target,
layer selection and selection history reject stale results. Native affine and
Color Range sheets display their candidate coverage while retaining the exact
native input for Apply.

## Auxiliary mattes and native version 25

The platform decodes and orients camera/host still auxiliary planes. OneComponent8 is
promoted exactly by 257; native OneComponent16 is retained; finite float coverage
is quantized once. Depth/disparity is explicitly normalized over its finite scene
range, with unknown samples outside coverage and near-white polarity. This is a
coverage interpretation, not a claim that a byte matte gained source precision.
Core resampling aligns an oriented matte to the canvas through the existing
native bilinear scalar sampler.

Each imported channel retains a bounded provenance record stating source format,
source coverage precision, orientation, oriented dimensions and alignment policy.
Malformed present auxiliary records report an error rather than silently losing
a matte. Native RAW, retained host still and preserving ordinary-image intake
share this typed attachment seam. Selected movie frames retain their decoded
alpha; a still photo's auxiliary matte is not inferred for a different frame.
The qualified HEIC auxiliary fixture has genuine partial alpha and decodes to
the existing RGBA16 host layout. Its opaque RGB10 packed variant remains an
explicit existing host-layout refusal, assigned to the broader 11D format work;
9G introduces no new packed RGB decoder. Provenance describes the decoded
auxiliary buffer precision; encoded-container
quantization remains explicitly uninspected.

Native document version **25** wraps a prior supported typed body and adds the
validated channel-properties tail. Readers retain prior formats; channels with
default display properties and no provenance keep their existing writer.
Metadata, raw native samples, channel identity, options and provenance survive
Undo and native/snapshot/recovery reopen. Channel replacement preserves identity
and metadata; duplication creates a fresh identity beside its source.

## Qualification

The final production app was built with normal release LTO. Its executable
SHA-256 is
`9ac0804e5a457b05cc995bd5590a7e95b6156bda675aac5f99987e1dd1527fdd`;
all **654 production inputs** matched the freeze. Authoritative Swift typecheck,
`make app`, **1,354 Rust tests** (zero failures, two existing ignored tests,
137 suites) and required Clippy pass. The final core archive SHA-256 is
`07d07e3be4813fd9b39425c4ad06cb82caf4b57e45a6362ebacbee53114712a7`.

The production MCP probe passes seven byte workflow groups/six byte scalar
filters and twenty native workflow groups/**31 native scalar filters**. Eighteen
scalar Auto cases cover all three commands, both analysis scopes and channel,
independently offset mask and alpha targets. Reported native endpoints reproduce
the exact published result when applied as Levels; the independent Auto output
oracle has zero code error. Selected filter comparison has a maximum one-code
error against its separately quantized full-result reference, with zero-coverage
and other-plane samples exact. Selection generation, affine/morphology, Quick
Mask/Actions, scalar brush/fill, all RGBA components, Apply Mask, channel math,
display and native/snapshot/crash recovery pass. Receipt:
`/private/tmp/rz-phase9g-qualified-mcp.json`, **816 unchanged inputs**, exact file
set, all owned applications exited. The generated reference matches **181** live
MCP tools exactly.

The actual-source UI probe passes **41 groups**, zero failures and zero remaining
owned documents; its process exited. It checks real brush/bucket/morphology and
tone controls, precise Channel Options, target/view Undo, native selection
sheets/gestures, Apply Image/Calculations/luminosity previews and Actions, native
RGB/mask thumbnails, all eight CV scalar formats and AV EXIF orientations, and
six raw scalar histogram targets. Twenty-four scope/stride cases each pass direct
and MCP exact integer bin weights, code sums and endpoint counts. Stale histogram
delivery refuses a changed target. The undersized channel-math token refuses,
while the planned compound job matches independent arithmetic. Owned captures
were inspected for visible controls, populated previews and thumbnails.

Normalized nonzero histogram plot doubles have a **4-ULP JSON/Foundation
transport allowance**, with **2 ULP observed** for RGB and zero for
channel/mask/alpha. Zero remains exact. Native integer statistics and all source
samples retain their exact gates; this allowance applies only to normalized
display values. Auxiliary provenance compares semantic numeric values across
Foundation/Rust JSON serializers, then the exact core-stored string after native
reopen. Receipt: `/private/tmp/rz-phase9g-ui-final5.json`, **405 unchanged inputs**
and exact file set; probe binary SHA-256
`b22aab2f693ae3fc907549b8036bddb604c58b7598601786700bd6e4555a04f6`.

The independent compatibility probe passes against genuine Phase 9F native
documents/editing-state packages and three original byte scalar outputs. It
reads legacy editing-state v1/v2, independently injects native v3 coverage and
target/view state, and checks exact Undo/reselect/Quick Mask behavior. Twenty
malformed package cases refuse invalid version/policy, unsafe file naming,
wrong/odd/overlong/empty coverage and invalid display/target state. Its final
receipt is `/private/tmp/rz-phase9g-qualified-compat.json`: 815 unchanged inputs and
all owned applications exited.

The independent ImageIO/container auxiliary probe passes all eight EXIF
orientations for both Depth and Portrait Matte channels: **zero native-code
error** against independently decoded payloads, orientation, normalization and
bilinear alignment. Native v25, Undo/Redo, native reopen, snapshots and
controlled crash recovery remain exact. The decoded inputs contain 247 and 252
distinct values respectively; aligned outputs contain 1,760 and 2,042. These
extra interpolated levels do not imply extra encoded source precision. Its final
receipt is `/private/tmp/rz-phase9g-qualified-auxiliary.json`: 842 unchanged inputs,
the declared opaque RGB10 layout refusal, and all owned applications exited.
