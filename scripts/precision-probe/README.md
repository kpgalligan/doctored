# Internal high-depth host probe

`make gate` runs the small run (no `--large*` flag) as its `precision-probe` step,
with `DR_BACKGROUND=1`, whenever the app sources, the header, this directory or the
core library changed since its last pass. By hand, after the coordinated release
Rust library is current, run `python3 -B scripts/precision-probe/verify.py --keep`
on macOS (`--keep` keeps the scratch directory after a pass); the `--large*` modes
below measure time and memory and run only by hand. This compiles the
actual app sources and exercises the internal typed wrappers and captured
save/archive path. Phase 7E also exercises the real ImageDocument owner, central Undo, public editing-state
archives and the private recovery writer/reader. It does not launch a separate
MCP server; the built-app MCP harness qualifies the visible tool surface.

Fixtures contain genuine 16-bit values, tiny nonzero alpha, hidden RGB, the system
Display P3 profile, non-square PPI and XMP. A standard-library PNG writer and
independent PNG/TIFF readers check exported storage, numeric output and metadata.
Swift checks immutable captured revisions, group sources, strict archive guards,
weak resource registration, deliberate byte previews and public typed-native
dispatch under a misleading extension. The legacy Rust decoder still refuses
typed native files; the facade selects the preserving typed decoder explicitly.

Phase 7C adds a genuine 65,536-value ramp. Severe Levels, continuous Curves and
64 live brightness increments smaller than one u16 code are checked against
independent f64 expectations at `2e-6 + 0.5/65535`; a destructive increment must
round only at its committed source. A separate standard-library ICC matrix/TRC
reference reads the actual system P3/sRGB tags, checks converted RGB within two
u16 codes and requires exact alpha, including hidden RGB processing. Assignment
must preserve every source code. Profile changes retain live parameters and
prepare their evaluation in the destination profile.

The probe writes a native v11 fixture directly from its published framing and
also writes the actual v12 framing and loads v1/v2 host archives; it does not relabel a current file to simulate backward
compatibility. Current native v13 adjustment records and v2 host archives must preserve
captured old parameters through later edits, native reopen and flat export.
Both isolated and pass-through adjusted groups have independent numeric checks.
Captured edit payloads have weak, conservative retention leases; copying a
capture shares its lease, independent captures are counted separately, and all
leases must disappear when their captures are released. Invalid evaluation tags,
target kinds, profiles, cancellation and insufficient budgets refuse explicitly.

`--large` additionally streams a 6000×4000 PNG16 fixture and starts a fresh process
for the same typed source/group/projection/native/export/archive workflow. All
stages retain original dimensions and the fixed 2 GiB reference limit the runner sets. Read
`files/large-report.json`: any refused stage makes `complete_workflow` false.
Runner exit 0 proves the probe completed, not that 24 MP met the size target.
The large run checks 257 distributed native source pixels; the small run checks
every source sample and independently decodes both exported formats.
The large grouped projection uses 257 distributed samples against an independent
f64 source-over reference at the frozen `2e-6 + 0.5/65535` error bound; its explicit
byte preview must match nearest rounding from the actual typed projection.

`--large-color` runs a separate fresh process for Phase 7C on the same 24 MP source.
Its exact live structure is Curves with endpoints 0/204 at opacity 0.5, an isolated
group at 0.8, calibrated Exposure +0.25 stops at opacity 0.75, and a pass-through group
at 0.9. Native/archive reopen must retain the raw source and live descriptions.
After releasing those owners, a fresh source receives destructive Levels gamma 0.8
and a P3→sRGB conversion, followed by native reopen. Each transition retains its
old source until the new result exists; projections, transient captures and
reopened owners are released between stages. The 12-stage
`files/large-color-report.json` names each retained scope, estimate, timing and
refusal. It includes 257 independent scalar source/tone/color samples: calibrated
RGB/ICC error is at most two u16 codes, destructive Levels one code, alpha exact
for stored sources and within the established render bound for the composite.
No refusal is substituted with a smaller image. Process RSS is measured separately.

Phase 7D checks genuine u16 masks/channels, independent mask placement and
properties, exact permutations/nearest geometry, and bilinear sampling against a
separate f64 oracle. Sparse paint/erase/clone use continuous authored RGB and
u16 coverage; local healing has an independent harmonic-boundary oracle. Actual
TextKit and shape paths produce scalar alpha coverage, explicitly promoted by
257 from 256 levels, while their colored raster output is assembled once in
Rust at u16 depth. Native v13 and typed archives preserve every scalar sample;
v11/v12 compatibility and refusal of authoritative byte APIs remain checked.

Phase 7E adds five public host groups: exact typed owner/readout versus byte display,
central history and explicit mode conversion, v2 editing-state/native archives
and v1 byte compatibility, saved identity and stale worker publication, and mixed
resource unions. A private recovery set writes both current typed state and a
named typed snapshot, counting their independently reopened facade metadata.
Timers change a target and then a document through the main run loop while owned
workers wait; their old results must refuse publication. Actual NSDocument Save
and the captured-old completion marker are checked separately. No stored user
credentials or recovery directories are accessed.
Nested Undo/Redo from a synchronous notification during real history publication
must be ignored; normal Undo/Redo afterward remains exact. Conversion from an
actual mask or saved-channel target restores that target on Undo. Legacy v1
archives ignore unknown later export fields and normalize them before writing v2.
Both byte and typed documents keep default event grouping enabled while nested
central edits, actual Save success, filesystem failure and subsequent Undo/Redo
are checked. The test does not bypass AppKit saving or disable event grouping.
Actual controller-created byte and typed windows also close through the window
button path; normal AppKit event turns have a fixed two-second bound to release
their document and every tracked pixel owner. Closed views may remain cached by
AppKit, so the check measures pixel ownership independently of view lifetime.
Preserving public export reuses its captured authoritative projection. An old
capture exported after a later opacity edit must retain its exact u16 pixels;
independent PNG/TIFF readers verify both revisions and their original metadata.

`--large-public` adds the separate Phase 7E public 24 MP workflow. A real
ImageDocument and window retain their current typed projection, saved owner and
history during preserving open, one sparse typed paint, Undo/Redo, native Save,
native reopen and PNG16 export. Nine stages keep the original 6000×4000 geometry
and the fixed 2 GiB reference admission limit the runner sets. The paint has an independent f64 reference;
257 distributed samples plus the painted samples check original/native u16
values, exported alpha and the exact profile. Every resource-report failure,
refused stage or remaining owner fails `files/large-public-report.json` and the
runner's qualification validator. Before/after resource snapshots are not peak
scratch or an OS memory ceiling. Run this fresh process only when other builds
and app probes have finished; do not substitute an older internal large workflow
for the actual editor's additional retained history and display allocations.
Temporary-document and final-window close settling times are included in their
stages. Exact export retained/scratch/total admission values are recorded; the
original history, current projection and display remain retained through export.

`--large-pixels` adds a separate native24MP Phase7D process. Its 21 stages use
one source with a genuine u16 mask and channel, a mask translation, exact whole
turns, full-source bilinear affine, native/archive persistence, then a fresh
source for sparse paint/erase/clone, bounded nonlinear local healing, and small
text/shape rasters. Each transition retains the old owner until the new owner
exists; scalar inputs, plane reads and reopened owners are released between
stages. There is no resized substitute. `files/large-pixels-report.json` records
scope, admission refusal, numeric checks and before/after retained-resource
snapshots. These snapshots are not a peak scratch measurement. Process peak RSS
is reported independently and the 2 GiB reference working limit the runner sets
is not an OS ceiling.

`--library /path/to/libdoctored_core.a` selects the coordinated release archive.
The runner checks its hash before/after a private immutable copy and links only
the copy, recording source/header/fixture hashes beside the build log. This
prevents concurrent Xcode regeneration from changing the library while linking.

Build and execution logs, process RSS from `/usr/bin/time -l`, images and private
settings remain under the reported temporary directory. The probe uses a private
bundle identifier, dummy API key and isolated Actions/recovery directories. It
starts no MCP server and touches no existing app process. Source-probe timings
use Swift `-Onone` with release Rust; they are not optimized UI latency claims.

Core admission independently validates actual typed records and operation
allocations. The archive manifest is admission guidance; its decode estimate is
not continuously reflected as the shared worker's live reservation. Payload and
dependency-work estimates are separate from measured process RSS, which can
retain freed allocator regions.
