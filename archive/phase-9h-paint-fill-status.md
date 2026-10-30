# Phase 9H — native paint and fill

Status: **P7-13/P7-14 verified** within the bounded native paint/fill scope.
The carryover ledger now has **18 verified and 18 open** assignments. The original
refinement/color-decontamination gate remains P7-12/9I; this does not close all
of Phase 9.

## Native stroke and replay contract

New native canvas strokes record the deposits the gesture actually produced,
including pressure-dependent diameter and opacity and every 20 Hz airbrush
Timer deposit. Smoothing still uses the existing canvas spline and leash. The
pixel brush retains snapped one-pixel squares; pressure opacity also applies
to its moving deposits, including exact zero-pressure no-ops. Mouse pressure
is one. Pressure Opacity is an additive native option; its optional persisted
field preserves existing tool preferences.

The RZBR version **1** Action payload contains `version: 1`,
`encoding: "le_f64_xy_diameter_opacity"`, base64 `dabs`, `hardness`, `roundness`,
`angle_degrees` and `pixel`. Each dab is four little-endian binary64 values:
canvas x, canvas y, diameter and deposit opacity. The deposit includes flow and
pressure opacity; stroke opacity remains separate and applies once. Positive
angles are counterclockwise on screen, matching the existing tip controls.
Unknown versions, fields, malformed numeric/boolean types, nonfinite geometry,
incomplete payloads and excessive counts refuse before publication.

MCP callers can pass `stroke_version: 1`, ordinary `points`, optional per-point
`pressures` and elapsed `times`, `pressure_size`, `pressure_opacity`, `airbrush`
and total `duration`. Resolution precedes dispatch and Action recording; the
saved command carries the canonical dabs. Callers do not need to construct
binary payloads. Times start at zero and never decrease; duration includes all
samples and any final hold. The endpoint is interpolated at each 0.05-second
airbrush tick. UI recordings retain actual event/timer deposits rather than
trying to reconstruct them from timestamps later.

At 8 bits, historical unversioned point calls and saved Actions retain the
original platform byte-footprint rasterization and its rounding; at 16 bits
(13A-6c-2) they generate native dabs through the same walk as
`stroke_version=1` and refuse by name at the 50,000-dab and 256-million-work-unit
limits. The legacy byte
painting, eraser, fill, gradient, dodge/burn and retouch kernels are unchanged.
Native generated coverage is an additive path, with a separate bounded FFI dab
array; the shared **64 KiB** edit-request limit is unchanged.

Soft ellipses evaluate the shared smoothstep field at pixel centers in
binary64. Hard ellipses use an explicit version-1 **8 × 8 area approximation**;
this is a geometry approximation, not byte alpha storage. Deposits accumulate
continuously and round once into u16 coverage. Authored RGB remains continuous
in explicit sRGB or document space, independent of display and coverage. The
native canvas overlay is a display preview; it is never the commit source.

## Paint, fill and scalar behavior

Native RGB brush painting supports all **27 existing paint blend modes** through
the shared blend kernel. Layer and group blending remain their separately
qualified workflow. Eraser remains destination-out, has no blend mode, changes
alpha alone and preserves hidden RGB. Dodge/Burn uses the existing six
range/direction combinations, preserving alpha. Linear and radial gradients
retain independent continuous RGBA endpoints, profile interpretation and opacity.
Masks, saved channels and all four RGBA planes support scalar gradients with
separate endpoint alpha and selection gating.

Brush, selection fill, color-match bucket fill and gradient can grow raw layer
storage wherever new canvas ink changes a previously absent pixel. Unchanged
storage, outside-canvas raw samples, layer metadata and independent mask
placement remain exact. Erasing absent storage is a no-op and creates no Undo
entry. The planner reserves the conservative raw/canvas union; final output
expands only to actual changed ink.

Bucket matching uses the existing anchored, four-channel tolerance algorithm
at native precision. UI and explicit `fill_mode: "color_match"` cover contiguous
and noncontiguous modes, including transparent absent storage on the canvas.
Tolerance keeps its historical 0–255 units and supports native fractional
thresholds. `fill_mode: "selection"` fills the selected area directly. Without a selection,
RGB fills the canvas; a scalar target fills its complete raw storage, including
stored samples outside the canvas.
Historical typed `contiguous: false` without `fill_mode` keeps its earlier
selection-wide meaning and raw-storage behavior. New color-match gestures and explicit calls require positive selection coverage
at the seed; a zero-coverage seed is a no-op even when selected matching pixels
exist elsewhere. Native code 1 remains eligible. Historical scalar bucket
calls from 9G retain their original seed-admission behavior; new scalar UI
recordings name the explicit color-match mode.

New paint/erase/dodge and retouch requests transfer generated stroke and native
selection as separate planes. Their final writeback multiplies the independent
gates before storage rounding, retaining tiny selection values. Clone uses this
continuous product. Healing/Spot Heal keep the existing half-coverage stroke
contour for the synthesis domain, intersect it with positive selection coverage,
and apply the exact native selection independently at final writeback. Legacy
retouch requests keep their prior combined-coverage behavior.

Native Quick Mask records canonical dabs without allocating a full-canvas RLE
Action. ImageDocument captures the document, revision, target, selected layers,
channel view and selection before the first worker. Native brush/selection work
is admitted and cancellable; stale results refuse and every completion/error
releases the canvas session. Historical six-byte and native eight-byte RLE
Actions remain accepted with their established meanings. Quick Mask remains
selection-only and file-clean.

## Bounds and qualification

Native generation and replay explicitly cap at **50,000 dabs**, 10,000 authored
points, host diameters 1–512 pixels and 256 million rasterizer work units.
Requests beyond the limits fail atomically rather than publishing a prefix.
The hard-edge 64-sample work is included in admission. Source, canonical payload,
output staging and working arrays participate in existing resource accounting.
Single-use job tokens provide progress/cancellation. Native document/history
formats need no new version: pixels and scalar planes use existing native
storage, and versioned tool arguments live in Actions.

The full Rust regression suite passed **1,369 tests**, zero failures and two
existing ignored tests across 138 suites. The final seed-admission correction
changes only the new native bucket branch; its affected FFI suite now passes
**16 tests**, including excluded-seed and native code-1 references. Required
all-target Clippy and formatting pass. The dedicated FFI cases independently check every
paint blend, soft coverage, pressure-equivalent accumulation, partial/tiny alpha
and separate selections, raw offsets/expansion/mask placement, bucket matching,
gradients including scalar endpoint alpha, six Dodge/Burn modes, cancellation,
work limits, NULL/error atomicity and counterclockwise squashed-tip rotation.
New selected clone, membrane healing and Spot Heal tests preserve native
selection code 1. Existing scalar and retouch compatibility tests pass.

The actual-source UI helper passes **nine groups** with unchanged recorded source
inputs. Synthetic tablet NSEvents pass through the real canvas handlers; the
probe records the actual pressure delivered by AppKit rather than claiming
physical-tablet fidelity. It checks 15 pressure-varying dabs and 2,048 changed
native components independently, five actual held Timer deposits for both the
ordinary and one-pixel brushes, and 19 exact zero-pressure pixel deposits.
Quick Mask produces 922 distinct native coverage values, matches an independent
scalar reference and replays exactly. Actual Cancel is observed at progress
0.028125 with 5,669,888 reserved bytes, draining to zero; a document/selection
change during the first coverage worker refuses publication. Native, named
snapshot and private recovery roundtrips retain exact samples and selection.

Actual compiled controls cover the full 27-entry blend popup with a Multiply
stroke, bucket Fill, linear/radial Gradient and Burn/Highlights, with exact
Actions and one Undo. A deliberately chosen Fill alpha/opacity boundary verifies
that independent float parameters produce native alpha 132; collapsing them
before transfer would incorrectly produce 133. Fourteen malformed generated
and canonical stroke cases refuse. Four additional actual Fill clicks (RGB/red plane, contiguous/noncontiguous)
verify excluded-seed no-ops with no Undo, dirty change or Action; seed code 1
remains eligible and records the explicit mode. The final helper receipt is
`/private/tmp/rz-phase9h-ui-final.json`, with its exact source-file set and hashes,
zero remaining worker scratch and an exited owned process.

The external MCP probe independently evaluates a 639-level rotated soft
footprint, generated pressure/timing and native erasure, with maximum native
code error 1 and exact outside coverage/latent RGB. All 27 blend modes, both
gradient kinds/endpoint-alpha cases, bucket modes and six Dodge/Burn combinations
replay through recorded Actions, Undo/Redo and native/snapshot/controlled crash
recovery. The final external witnesses pass all six scalar targets with **zero numeric
error** for brush writeback and exact gradient Action replay, excluded-seed
no-ops and the Fill parameter-rounding boundary. Its receipt is
`/private/tmp/rz-phase9h-qualified-mcp.json`: no source changes and all owned
applications exited.

The Display P3 wiring reference uses the same explicitly assigned **built-in
sRGB** profile as authored sRGB ink, followed by the independently qualified
profile-conversion operation. The fixture's macOS system sRGB ICC has a different
quantized matrix/TRC payload and is not substituted for that authored profile.
The declared two-native-code counterpart bound is unchanged; observed brush,
fill and gradient differences are 0, 1 and 1 respectively, with exact profile
and alpha. This is a host wiring reference to the existing qualified transform,
not a newly claimed independent color-conversion algorithm.

The separate compatibility script uses 44 byte outputs from the unchanged
Phase 9F app and 16 typed/unversioned outputs and old Actions from the unchanged
Phase 9G app. All **60 cases** pass the final frozen Release candidate exactly, including
original selections, direct calls, imported Actions and Undo/Redo; expected
outputs are never regenerated from the candidate. The final receipt is
`/private/tmp/rz-phase9h-qualified-compat.json`: **826 unchanged inputs**, exact
file set and all owned applications exited.

The authoritative current-source Swift typecheck, normal-LTO `make app` and
required all-target Clippy pass. The final production executable SHA-256 is
`28539cc7632767d7699f0543fcdbbb3e6cff5403a6fd6db1bdd6a7aa261c1ac8`;
all **664 production inputs** remained unchanged. The matching production
archive SHA-256 is
`a75c94a25105ec453defc7b5bfcfb316a0d0ceb447c96d4ae8fb6377aa400681`.
The final public catalog still contains **181 tools**; native behavior uses
additive arguments on the existing UI-mirroring tool names.

Larger or tiled strokes, larger history/canvas targets and broader resource
qualification remain **13A/13D**. This phase pulls forward the stroke replay
foundation needed from **12A**, without claiming all remaining replay work.
The original refinement/color-decontamination gate remains **P7-12/9I**.
