# Phase 7A: high-depth representation and prototype

This slice compares storage and evaluation without exposing a document mode.
The existing editor, C ABI and native v10 remain RGBA8. The safe Rust prototype
is deliberately a different type, so an existing byte-based filter or writer
cannot accept it by accident. The [operation matrix](phase-7-operation-support.md)
records the migration required before public editing.

Phase 7 depends on Phases 3 and 4, which are implemented. Starting it after the
user's request to continue to the next phase does not complete or waive the
[Phase 6 cutout gate](phase-6-selection-mask-status.md).

## Evaluation contract

| Boundary | Decision |
| --- | --- |
| Stored pixels | Compare straight RGBA16 unsigned normalized samples and straight RGBA32 normalized floats. Row 0 is top, tightly packed, all four channels use the declared type. Immutable `Arc` ownership preserves sharing. |
| Processing | Reuse the existing f32 W3C compositor in `blend.rs`. All prototype group intermediates retain f32 precision; quantization happens only at an explicit output storage boundary. Accumulation error is measured against independently expressed f64 premultiplied source-over. |
| Profile | Every prototype image and scene carries an `Arc<IccProfile>`. Compositing requires clones of the same captured profile handle, a deliberately stricter prototype rule than byte-equivalent ICC matching. This bounds profile validation without scanning large independent profiles. No implicit conversion or automatic adoption of the app working space. Encoded RGB stays encoded. |
| Alpha | Stored alpha is straight normalized coverage, independent of the RGB transfer curve. Transparent source pixels retain their hidden RGB in storage. A fully transparent rendered composite has zero RGB because no source contributes. Alpha multiplication/compositing occurs in f32. A positive effective alpha below `2^-32`, including a positive product that underflows to zero, explicitly refuses rendering; otherwise the shared kernel could lose straight RGB through underflow. Float storage and same-float conversion still preserve smaller alpha; integer/byte downconversion rounds it. Future spatial kernels must share premultiplied sampling and unpremultiply at storage boundaries. |
| Float range | Finite `[0,1]` values only, including alpha. Reject NaN, infinity and extended range. This is a precision comparison, with no HDR or linear-light policy. |
| Quantization | Explicit nearest rounding after clamping numerical output to `[0,1]`: `round(channel * 65535)` or `round(channel * 255)`. A typed float source is validated, rather than silently clamped on import. No dithering in the prototype; delivery dithering belongs to 7E. |
| Preview | Explicit RGBA8 conversion creates a separate image. It is never a replacement source, typed layer or native save input. This is a byte conversion, not an ICC display transform. |
| Legacy files | Preserve established byte storage, encoded compositing, isolated-group byte rounding and versioned `document_v2` adjustment behavior. Do not route old documents into the prototype and change their appearance. |
| Persistence | No prototype serialization or native version bump in 7A. 7B must add explicit storage/evaluation tags with a version bump, backward readers and shared writer/reader limits. Groups in the high-depth mode must avoid legacy byte intermediates. |
| Masks and channels | Prototype masks/channels are unavailable. Existing selections/masks/channels remain u8. Plan high-depth persistent masks and saved channels as u16; byte coverage can be promoted exactly by multiplying by 257, with its original 256-level limitation disclosed. A high-depth pixel alpha or color-plane edit must never pass through a byte mask/plane API. |
| Conversion | Byte-to-u16 promotion is exactly `v * 257`; u16-to-f32 is `v / 65535`. Conversion preserves the profile and does not add source information. Converting old editable documents also needs an explicit evaluation policy; merely changing storage cannot preserve all old group/adjustment semantics. |

The prototype supports canvas placement, opacity and Normal raster/isolated-group
compositing. It does not implement an alternative adjustment suite. In particular,
the existing ICC `ToneEncoder` returns byte-level values even when called by a
float adjustment interpreter. Phase 7C must generalize that shared encoder and
requalify curve/matrix tolerances; passing floats through its present interface
does not preserve high depth.

Prototype construction/render/conversion methods return image values, not
document edits. A same-storage conversion shares the input allocation. A future
UI/MCP conversion command must detect that no-op before the central mutation
path so it cannot register an empty Undo step.

## Comparison protocol

The targets below are fixed before running the prototype measurements. Run:

```sh
cd core
cargo run --release --example phase7_precision > /tmp/rz-phase7a-precision.json
```

`--accuracy-only` omits large-frame measurements. The checked-in example supplies
true 16-bit source samples rather than upsampling an eight-bit fixture. Its
65,536-pixel opaque red ramp enumerates every possible u16 code; two isolated
group boundaries must retain those input values exactly. Separate fractional
alpha/color cases compare flat and grouped compositing with an independent f64
reference, using the actual typed source values. The maximum absolute error
limit is `2e-6` for f32 output, and `2e-6 + 0.5/65535` for u16 output. The latter
adds exactly the rounding allowance of the selected storage type.

A separate numerical diagnostic applies 100 hypothetical increments of `1e-6`,
rounding at each simulated storage write, and compares with the independent
unclipped result `original + 0.0001`. Each increment is smaller than half a u16
code. It measures the loss of repeated small edits to integer storage; it does
not qualify an editor adjustment or introduce a production tone kernel. This
protocol was chosen before measurement: a multiply/divide pair with a factor
above one can undo its own integer rounding and does not test cumulative loss.

Performance uses 6000 × 4000 and 8000 × 6000 synthetic frames, two independent
layer allocations, and the same deterministic u16-code pattern in either store.
One case composites two ordinary layers; the other puts the top layer in an
isolated group. Each admitted case renders three times serially, releasing each
output, and reports all times plus the median. Timed rendering includes planning,
allocation, blending and output storage; input construction is reported separately.
257 distributed output pixels are also checked against the independent reference.
These are encoded-color numerical fixtures, not photographic quality samples.

The provisional neutral-render latency target is a median of 2 seconds at 24 MP
and 4 seconds at 48 MP. The working limit is 2 GiB, matching the app's upper policy
bound. A size/structure refused at that limit fails that size target; no resized
substitute or increased budget counts as a pass. A successful small prototype
does not establish large multilayer production support. Phase 13 must be pulled
forward where these measurements prevent the intended workflow from fitting.

## Memory and execution

| Retained RGBA payload only | RGBA8 baseline | RGBA16 | RGBA32 float |
| --- | ---: | ---: | ---: |
| Bytes per pixel | 4 | 8 | 16 |
| One 24 MP layer | 96,000,000 | 192,000,000 | 384,000,000 |
| One 48 MP layer | 192,000,000 | 384,000,000 | 768,000,000 |

These exact payload sizes exclude masks, history, projections, group scratch,
profiles and platform allocations. The prototype plan counts unique shared
retained buffers through the existing resource inventory, then reserves its
conservative output and f32 accumulator requirements. The caller supplies any
additional retained work. Neither payload accounting nor admission is an OS
memory ceiling.

The prototype limits scenes to 1,024 records, 16 group levels and two billion
pixel visits. Each isolated group currently allocates a full canvas accumulator;
these limits do not establish regional rendering. A 1 MiB allowance covers
bounded inventory/tree and Arc bookkeeping. Allocator size-class/page rounding
and retained residency are not measured payload and have no exact bound here. Constructors adopt already allocated caller vectors, so callers remain
responsible for admitting source decode/allocation before invoking them.

Each render uses the existing single-use job control, drops partial output on
failure, and releases reservations on return. Limits on dimensions, scene records,
group depth and pixel visits precede rendering. Dense work polls cancellation in
bounded chunks. This establishes a core work bound; it does not qualify a public
AppKit workflow or promise a scheduler/allocator wall-clock bound.

## Results and storage selection

Select **RGBA16 unsigned normalized storage with f32 processing** for the first
7B photographic document path. It preserves all 16-bit input codes and halves
retained raster memory relative to float storage. Keep group accumulators f32
and round once when producing an explicitly typed stored result. Retain the
float prototype for comparison; it does not establish a second public mode.

Float storage is more accurate between repeated small writes and was faster in
this neutral-render benchmark, so the integer choice has a real tradeoff. Future
live adjustment stacks should evaluate through float intermediates rather than
write integer storage after every live operator. Destructive u16 edits still
round at each committed write; do not advertise sub-u16 retention for that mode.

Measured September 10, 2026 on an Apple M2 Max, Mac14,6, 96 GiB RAM,
macOS 26.5.1 (25F80), Rust 1.92.0, using the unchanged optimized Release profile.
No other owned heavy workload ran during the recorded comparison. Source and
binary hashes are in `/tmp/rz-phase7a-manifest.json`; numerical/timing records
are `/tmp/rz-phase7a-precision.json` and `/tmp/rz-phase7a-precision-time.log`.

| Numerical result | RGBA16 | RGBA32 float |
| --- | ---: | ---: |
| Opaque 65,536-code ramp, two isolated groups | Every source value exact | Every source value exact |
| Maximum absolute error, ordinary layers | `7.73822e-6` | `1.01735e-7` |
| Maximum absolute error, isolated top layer | `7.72195e-6` | `1.22305e-7` |
| Predetermined error bound | `9.62951e-6` | `2e-6` |
| 100 hypothetical `+1e-6` writes: maximum error | `1e-4` | `1.35758e-6` |

Both neutral numerical gates pass. Integer storage discards each sub-code
increment in the diagnostic; float retains the accumulated change with small
rounding error. The diagnostic is evidence for the storage tradeoff, not a failed
or enabled adjustment implementation.

| Storage / two-layer scene | Median render ms (range) | Planned retained + scratch GiB | Result at 2 GiB |
| --- | ---: | ---: | --- |
| u16, 24 MP, ordinary | 286.9 (285.0–329.9) | 0.895 | Admitted; accuracy/latency pass |
| u16, 24 MP, isolated top layer | 385.6 (369.0–391.2) | 1.074 | Admitted; accuracy/latency pass |
| u16, 48 MP, ordinary | 578.1 (566.2–649.8) | 1.789 | Admitted; accuracy/latency pass |
| u16, 48 MP, isolated top layer | — | 2.147 | Refused before render pixel allocation |
| f32, 24 MP, ordinary | 210.2 (209.6–211.0) | 1.074 | Admitted; accuracy/latency pass |
| f32, 24 MP, isolated top layer | 280.9 (280.9–282.2) | 1.431 | Admitted; accuracy/latency pass |
| f32, 48 MP, ordinary | — | 2.147 | Refused before render pixel allocation |
| f32, 48 MP, isolated top layer | — | 2.862 | Refused before render pixel allocation |

All five admitted cases completed three runs and passed the sampled reference
checks. The other three cases failed their size targets through explicit memory
refusals. These results select an initial 24 MP neutral-path qualification target;
they do not justify dependable large multilayer high-depth editing. Pull regional
group evaluation and/or tiled backing forward from Phase 13 before qualifying the
48 MP grouped workflow. Do not raise the limit to make the current table pass.

The whole serial benchmark process reached **3,275,702,272 bytes maximum RSS**
and **3,276,704,408 bytes peak memory footprint**, above its live-payload admission
limit. A separate temporary driver, linked to the same Release library, recorded
requested live allocations through Rust's `System` allocator. The full sequence
peaked at **1,920,033,494 live bytes**, below the **1,921,057,881-byte** maximum
admitted plan. After all cases dropped, only 25,075 tracked Rust bytes remained.
`vmmap` identified approximately 3.0 GiB of still-resident, dirty `MALLOC_LARGE
(empty)` regions. An explicit diagnostic allocator-pressure-relief call released
zero bytes. This establishes residency of freed large allocations rather than
missing live image/group buffers for this run.

Fresh-process checks support that distinction: the u16 48 MP case reached
**1,922,039,808 bytes RSS** with **1,920,012,643 live requested bytes**; f32 48 MP
input construction followed by its render refusals reached approximately
**1.538 GB RSS** with **1,536,018,991 live requested bytes**. These diagnostic
runs occurred during regression compilation, so their timings are not used.
No allocator policy or production memory limit was changed. Full diagnostic
findings and source/VM logs are indexed by
`/tmp/rz-phase7a-memory-audit/findings.json`.

The 2 GiB figure must not be described as a process-memory ceiling or a qualified
total application footprint. Before qualifying a public mode, measure sequential
editing and decode/display/undo/recovery residency in the actual app; the current
experiment does not exercise that workflow.

The first run completed its numerical report but the sandbox blocked
`/usr/bin/time` from reading `kern.clockrate`, causing the timing wrapper to
return 1. Its report and log remain under `/tmp/rz-phase7a-precision-sandbox*`.
The recorded run had access to timing counters and exited 0. Both runs produced
the same numerical outcomes and capacity refusals; the table uses the final run.

## Final verification

The complete Release core suite passes **891 tests, 0 failures, 2 existing
ignored**; all nine prototype tests include the final review assertions. Rust
formatting and all-target Clippy with warnings denied pass. The numerical runner
exits 0 with the capacity refusals preserved in its report. Tests exercise genuine
sub-byte distinctions, independent f64 nested/mixed-storage compositing, all u16
codes, hidden RGB, invalid/profile/alpha refusals, capacity and shared allocations,
exact work bounds, and cancellation after compositing has begun. No existing
render kernel or persisted evaluation policy changed.

The [status](phase-7-high-depth-status.md) records logs, the operation matrix and
the next 7B assignment. No Swift or C ABI changed, and this slice does not expose
an application workflow or claim completion of Phase 7.
