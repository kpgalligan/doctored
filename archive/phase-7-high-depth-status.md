# Phase 7: high-depth editing status

Current downstream update: [Phase 9G](phase-9g-selections-masks-status.md#qualification)
verifies P7-08–P7-11 with native scalar editing/display, source-derived selections,
authoritative selection/Quick Mask coverage and typed auxiliary mattes.
[Phase 9H](phase-9h-paint-fill-status.md) verifies P7-13/P7-14 with native
pressure/airbrush replay, paint and fill parity. The bounded
[Phase 10D blending slices](phase-10d-threshold-modes-status.md) add native raster and
isolated-group blend modes and Pass Through transitions with conditional
v26/v27/v30/v31/v33/v34/v37 persistence, composed with outside-mask29, and the
[Blend If slice](phase-10d-blend-if-status.md) adds typed Blend If v35 and the
[layer-styles slice](phase-10d-layer-styles-status.md) typed layer styles v36 (verifying P7-20); the
[adjustment-blending slice](phase-10d-adjustment-blending-status.md) gives typed adjustment layers
the same twenty-six modes with conditional v37/schema7, so P7-19 is verified by the
adjustment-blending slice; knockout is unimplemented at both depths and is tracked as P7-37.
Twenty-one carryovers are verified
and 16 remain open; P7-12/9I's original cutout quality gate remains open.
Earlier Phase 7/8 inspection, selection/scalar and blend boundaries below are
historical; use the [current support matrix](phase-7-operation-support.md)
for enabled workflows.

Phases 7A–7E are complete within the qualified supported workflow, including the
public 16-bit editor mode. This is
the independent precision phase requested after the
Phase 6 continuation; the [Phase 6 quality gate](phase-6-selection-mask-status.md)
remains open. The implementation plan explicitly allows the precision foundation
to proceed after Phases 3 and 4.

[Phase 8 source work](phase-8-retained-source-status.md) adds embedded/linked
originals, affine rerenders and explicit cross-depth fallback conversion in both
modes. Its qualification, native assets and later boundaries are recorded separately.

| Slice | Status |
| --- | --- |
| 7A: representation decision and prototype | Complete: typed prototype, numerical/resource comparison, storage decision and operation matrix. Select u16 storage with f32 processing for 7B. |
| 7B: complete neutral path | Complete internally: genuine 16-bit PNG/TIFF, immutable typed layers, isolated/pass-through groups, v11 native save/reopen, shared metadata policy and typed captured saves/archives. All nine stages of the declared 24 MP workflow pass; scope and RSS limits below. |
| 7C: shared tone/color processing | Complete internally: all 21 shared adjustments, continuous ICC, typed live/destructive edits, v12 persistence with v11 reads and captured edits. The declared 24 MP color workflow passes; [contract and limits](phase-7-tone-color.md). |
| 7D: geometry, paint and retouch | Complete internally: u16 masks/channels, shared geometry, continuous paint/erase/clone, bounded healing, scalar TextKit/shape rasterization and native v13 with v11/v12 reads. All 21 stages of the declared 24 MP workflow pass; [qualification and 7E handoff](phase-7-geometry-paint-status.md). |
| 7E: public document mode | Complete within the public support matrix: preserving import, typed editing/history/recovery, explicit conversion and export dithering. 999 Rust tests, 18 Swift + 2 codec groups, 25 MCP groups and all nine public 24 MP stages pass; [final status and limits](phase-7-public-mode-status.md). |

The [representation decision](phase-7-high-depth-decision.md) holds the evaluation,
precision, resource and measurement contracts. The
[operation-support matrix](phase-7-operation-support.md) identifies existing byte
boundaries and the required migration on both sides of the FFI.

The sections below retain the results and boundaries at each earlier milestone;
the [7E status](phase-7-public-mode-status.md) is the current public contract.
The [carryover ledger](phase-7-carryovers.md) assigns every remaining capability
and qualification gap to a required later slice; those assignments remain open.

The original prototype sources are `core/src/precision_image.rs` and
`core/src/precision_render.rs`; reproducible measurements are in
`core/examples/phase7_precision.rs`. There is no public high-depth document mode,
new C ABI, native format change or separate production adjustment suite in 7A.

## Implemented in 7A

- Immutable typed u16 and normalized f32 sources with one captured profile;
  explicit storage/byte-preview conversion, finite-input validation and no
  implicit entry into byte editing APIs.
- Normal raster/isolated-group compositing through the existing `blend.rs`
  kernel. Groups retain float intermediates; output rounds only at the selected
  storage boundary. Extremely small effective alpha explicitly refuses.
- Unique retained-buffer accounting, checked output/scratch admission, limits on
  graph size/depth/pixel work, and single-use cooperative cancellation.
- An executable prototype support query and a core/app migration matrix. The
  editor remains RGBA8; unsupported operations are unavailable in the prototype.

## Measurements and boundaries

Both stores preserve every code in the genuine 65,536-value u16 ramp through two
isolated groups. Independent f64 reference checks pass their predetermined bounds.
Float also retains small repeated hypothetical writes more accurately, while u16
halves retained pixel memory. The selected initial representation is u16 storage
with f32 evaluation, beginning with a 24 MP neutral-path qualification target.

All five admitted performance cases complete three runs and pass accuracy/latency
targets. The u16 ordinary/isolated 24 MP medians are 286.9/385.6 ms; ordinary
48 MP is 578.1 ms. The f32 ordinary/isolated 24 MP medians are 210.2/280.9 ms.
Both 48 MP isolated cases and ordinary 48 MP f32 refuse under the unchanged
2 GiB tracked-memory limit. Regional/tiled group work is required before
qualifying that larger grouped workflow.

The serial process reached 3.05 GiB RSS. A separate allocator/VM audit found
1.920 GB peak live requested allocations, within the plan, and freed large
allocation regions remaining resident after cleanup. This confirms that the
tracked limit is not a process RSS ceiling. Actual app decode/display/history/
recovery residency remains to be qualified; no allocator workaround is shipped.

## Verification

- The complete final Release suite passes **891 tests, 0 failures, 2 existing
  ignored**, across 75 result groups. This includes all nine precision tests and
  final review assertions for shared group buffers, numerical-refusal cleanup,
  successful token completion, exact work-limit boundaries and empty intersections.
  The optimized test build took 20m33s. Log: `/tmp/rz-phase7a-core-tests.log`;
  summary: `/tmp/rz-phase7a-core-summary.json`.
- Rust formatting and all-target Clippy with warnings denied pass. Logs:
  `/tmp/rz-phase7a-format.log`, `/tmp/rz-phase7a-clippy.log`.
- The numerical/performance runner exits 0; source,
  binary and result hashes are in `/tmp/rz-phase7a-manifest.json`. Reports:
  `/tmp/rz-phase7a-precision.json`, `/tmp/rz-phase7a-precision-time.log`.
- Independent process-memory evidence:
  `/tmp/rz-phase7a-memory-audit/findings.json`. All diagnostic processes exited.
- All 64 links in the new decision/status/matrix documents resolve. No Swift or
  C ABI changed; no public app workflow is claimed by these core experiments.

## Implemented in 7B

The preserving neutral path for the selected u16 representation now provides:

1. Explicit document/layer storage and evaluation, with distinct typed FFI and
   Swift owners. Legacy byte handles and rendering retain their prior contract.
2. Genuine RGB/RGBA16 PNG and classic TIFF decoding with orientation, profile,
   PPI, metadata and allocation preflight. Typed refusals cannot enter ImageIO
   byte fallback or automatic working-space conversion.
3. Isolated Normal and pass-through groups retaining f32 intermediates through
   the authoritative u16 projection. Duplicate/opacity/group edits preserve
   immutable sources; unsupported pixel operations remain unavailable.
4. Tagged native v11 records with shared writer/reader limits and complete record
   preflight before pixel decode; exact sources and metadata survive reopen.
   Independent PNG/TIFF readers verify actual 16-bit output and carried packets.
5. Independent genuine sub-byte, fractional-alpha and group references, separate
   legacy appearance regressions, typed captured saves and versioned neutral host
   archives. Public editing-state integration remains gated with the mode.

Keep the public document-mode gate closed until 7C/7D provide the supported
workflow matrix and 7E exposes intentional conversion and truthful limitations.
At the 7B milestone, Phase 7 as a whole was not complete; 7E now completes its
qualified public workflow. Phase 6's quality limitations remain open.

The [7B implementation](phase-7-neutral-path.md) uses separate `PrecisionDocument` / `PrecisionImage`
handles and internal Swift owners. Supported group boundaries are isolated
Normal and pass-through groups. Legacy `RzDocument` storage/evaluation stays byte-based, writes
v10, and refuses typed v11 before decoding. The internal preserving importer
has no ImageIO byte fallback or automatic working-space conversion. Captured
saves and versioned typed host archives retain the exact source handle; their
qualification does not claim public Undo/recovery integration. The latter is
required when the typed mode enters `ImageDocument` in 7E.

## 7B verification

The complete Release suite passes **911 tests, 0
failures, 2 existing ignored**, including all **20 new tests** and the existing
group regressions after the shared premultiplied gate extraction. Formatting,
all-target Clippy with warnings denied, the authoritative Swift typecheck,
Release library/app builds and complete small host probe pass. The optimized
regression build took 14m08s. Private app MCP smoke also passes: 140 existing
tools, exact v10 save/reopen pixels, grouping/filter Undo/Redo and typed v11
refusal through the byte editor. Logs use the `/tmp/rz-phase7b-` prefix; results and source
hashes are captured in `/tmp/rz-phase7b-manifest.json`, with the full test summary
in `/tmp/rz-phase7b-core-summary.json`.

The small host probe checks every source sample and uses independent PNG/TIFF
readers for exported precision, straight alpha, Display P3, non-square PPI and
XMP. It also checks captured-old saves, both group modes, strict host archive
guards and weak source/projection ownership. All 17 new C exports have NULL
coverage. No production source hash changed after the Release build.

## 7B 24 MP workflow and remaining limits

All **nine stages pass**, with no resizing or resource refusal, under the
unchanged **2,147,483,648-byte tracked working limit**. The fresh process uses a
genuine 6000×4000 PNG16 source, duplicated into two layers and an isolated group.
Those editing leaves share **one 192 MB source allocation**. Native reopen
decodes separately serialized leaves; this run does not qualify two independent
24 MP input documents or a general multilayer editor session.

Measured on the Apple M2 Max (Mac14,6), 96 GiB RAM, macOS 26.5.1 and Rust 1.92.0,
after owned compiler and app-smoke processes finished.

| Stage | Observed seconds |
| --- | ---: |
| PNG16 import | 0.760 |
| Duplicate/group | 0.001 |
| Grouped projection and full-size byte preview | 0.474 |
| Captured native save / reopen | 0.585 / 0.219 |
| PNG16 / TIFF16 export | 0.508 / 0.471 |
| Typed host archive save / reopen | 0.433 / 0.214 |

257 distributed source/projection samples match exact source values and the
independent f64 oracle at the unchanged `2e-6 + 0.5/65535` bound; byte previews
match integer nearest rounding. All typed handles, tracked retained bytes and
scratch return to zero at the end. This is one fresh source-probe run with
Release Rust and `-Onone` Swift, not repeated optimized UI latency qualification.

Process maximum RSS is **2,952,904,704 bytes (2.75 GiB)**; peak footprint is
2,873,609,096 bytes. The first positive startup RSS sample is 13,139,968 bytes,
which is not a warmed baseline. As in 7A, tracked admission is not an OS RSS
ceiling; this run does not repeat 7A's allocator attribution experiment. Actual
public display/history/recovery residency and larger workflows remain unqualified.

The preserved probe directory is
`$TMPDIR/rasterize-precision-probe-il06p6ts`.
It contains `files/large-report.json`, `large-process-memory.json`,
`large-run.log`, `large-run-provenance.json` and the independent reader outputs.
Reproduce with [the probe](../scripts/precision-probe/README.md). All owned test
processes have exited.

At the end of 7B, public mode and central Undo/recovery were unexposed, and
tone/ICC operations, masks, geometry, paint and retouch still needed migration.
7C now supplies the tone/color subset described below.
Codecs have stage cancellation, compound export is noncancellable, and PNG/native
payloads currently use uncompressed encoding. The 48 MP workflow remains
unqualified; regional/tiled work is still required before larger claims.

## Implemented in 7C

All **21 existing adjustment operations** now use the shared interpreter and
kernels in the separate typed workflow. Descriptions require top-level
`evaluation:"precision_v1"`. Curves evaluates continuous splines; calibrated
Exposure/White Balance use continuous ICC encoding; Shadows/Highlights retains
float alpha-weighted guides. Live stacks and supported groups retain f32
intermediates. Destructive raster application rounds RGB once to u16, preserves
alpha and leaves zero-alpha hidden RGB unchanged.

Typed assign/convert operations capture profile context. Assignment shares exact
samples; conversion transforms each distinct stored RGB allocation, including
hidden RGB, preserving alpha and sharing. Live parameters remain unchanged and
are rebound to the destination profile, so a nonlinear adjusted composite can
change appearance after source conversion. Exact profile/model identity is
separate from the old byte tolerances.

Native **v12** adds versioned adjustment records, with complete operator/record
preflight before raster decoding. Typed **v11** reads and legacy **v1–10**
evaluation/v10 writing remain supported. Captured edits, compact descriptors,
weak parameter/profile payload leases, and manifest-v2 archives with v1 reads
extend the existing worker/capture seam. No second Undo system or public mode
was added. See the [implementation contract](phase-7-tone-color.md).

## 7C verification

The final full Release suite passes **941 tests, 0 failures, 2 existing ignored**
across **85 result groups**, including **all 30 new tests**. Optimized compilation
took **15m47s**. Formatting, all-target Clippy with warnings denied, authoritative
Swift and complete probe typechecks, and the Release library/app builds pass.
All **409 production/header source hashes** match the validated snapshot. All
**eight new C exports** have NULL coverage.

Independent scalar references cover all 21 shared operators, using achromatic
special cases where appropriate; existing shared-kernel tests separately cover
nonneutral color behavior. The largest scalar error is `2.384e-7`, below the
unchanged `2e-6` float bound. Full-u16 Levels/continuous Curves peak at
`3.0471e-8` / `1.4843e-8`. Independent nonlinear Bezier/de Casteljau references
also pass over every u16 code, with noncommuting channel/master curves, tiny
alpha, nested groups and native reopen. The direct 2D Gaussian guide reference
peaks at `7.5794e-6` after storage, below `2e-6 + 0.5/65535`.

ICC uses a separate forward-grid bound of `1/65536 + 6e-8`; this does not relax
the compositor bound. Measured maxima are `3.00e-8` for severe gamma ramps,
`1.02231e-5` for parametric/pedestal cases, and `7.1327e-6` for sampled curves.
All five parametric kinds, terminal plateaus, exact identity, and invalid
source/destination profiles are covered.

The actual AppKit source probe passes **8 Swift + 2 independent Python groups**:
genuine ramps, live sub-code accumulation, exact captured-old saves, both group
modes, PNG/TIFF readers, profile conversion, native v11/v12, host archive v1/v2,
byte-editor refusal and payload release. Its ICC result differs from the
independent reference by at most one u16 code. The independent native framing
reader verifies persisted operator JSON. A test-only v11 PNG fixture initially
included forbidden metadata; correcting it to the published pixels-only format
required no production or Swift changes.

The isolated Release app MCP smoke passes **11 checks**: exactly 140 existing
tools, legacy group/filter Undo/Redo, exact native-v10 reopen, and typed-v12
refusal without changing open documents. Curves and calibrated Exposure have
zero byte-code error against their independent references; Exposure and
Shadows/Highlights live/destructive paths match exactly. All owned app/compiler/
probe processes have exited.

Logs and source/artifact provenance use `/tmp/rz-phase7c-`; the consolidated
manifest is `/tmp/rz-phase7c-manifest.json`, the full suite summary is
`/tmp/rz-phase7c-core-summary.json`, and host evidence is
`/tmp/rz-phase7c-host-qualification.json`. Xcode regenerated the static archive
from unchanged sources; the manifest records distinct build-stage hashes and
the source probe's actual linked archive, without claiming binary identity.

## 7C 24 MP color workflow and limits

All **12 stages pass**, with **no resizing, resource refusal or verification
failure**, under the unchanged **2,147,483,648-byte tracked working limit**.
The fresh process uses a genuine 6000×4000 PNG16 source. Its live structure is
Curves at opacity 0.5, an isolated group at 0.8, calibrated Exposure +0.25 stops
at opacity 0.75, then a pass-through group at 0.9. Editing revisions share one
**192 MB source allocation**. Native/archive reopening temporarily retains a
second independently decoded source. Those owners are released before a fresh
source receives destructive Levels and P3-to-sRGB conversion; each transition
retains its old source until the new one exists.

Measured on the Apple M2 Max (Mac14,6), 96 GiB RAM, macOS 26.5.1 and Rust 1.92.0,
after owned builds and app checks finished.

| Stage | Observed seconds |
| --- | ---: |
| Initial preserving import / fresh destructive source | 0.749 / 0.715 |
| Live Curves/Exposure/group preparation | 0.062 |
| Authoritative full-size live projection | 6.080 |
| Captured live native save / reopen | 0.302 / 0.089 |
| Captured live archive save / reopen | 0.221 / 0.098 |
| Destructive Levels gamma 0.8 | 0.776 |
| Stored-source P3-to-sRGB conversion | 3.678 |
| Converted native save / reopen | 0.226 / 0.074 |

257 distributed source/live/destructive/ICC samples pass independent f64
references. Calibrated live RGB/ICC stay within two u16 codes, destructive
Levels within one code, stored source alpha is exact, and rendered alpha meets
`2e-6 + 0.5/65535`. Final typed handles, payload leases, tracked retained bytes
and scratch are all **zero**.

The process took **13.206 s**. Maximum RSS is **2,577,252,352 bytes (2.40 GiB)**;
peak footprint is **2,492,697,192 bytes**. The first observed launch RSS is
13,090,816 bytes, **not a warmed AppKit baseline**. This is one fresh run using
Release Rust and Swift `-Onone`, not an optimized interactive-latency claim.
The 6.08 s full projection and 3.68 s conversion still need regional/preview
work before responsive public interaction. Tracked admission is not an OS RSS
ceiling; this run does not repeat 7A's allocator attribution experiment or
qualify 48 MP, arbitrary multilayer sessions, or public history/recovery.

Artifacts are under
`$TMPDIR/rasterize-precision-probe-9qdy54x1`:
`files/large-color-report.json`, `large-color-process-memory.json`,
`large-color-run-provenance.json` and the final small `files-final` directory.
Reproduce with [the probe](../scripts/precision-probe/README.md) and
`--large-color`.

Remaining typed limits: masks/scalar planes, styles/clipping/other blend modes,
geometry, paint and retouch are unavailable. The policy remains normalized
encoded-RGB SDR with destination gamut clipping. Authored colors and gradient
parameter granularity retain their existing schema; repeated destructive writes
still round to u16. Native reopen conservatively sums each operator's full
preparation allowance, so long simple stacks can refuse before 1,024 records.
ICC/JSON preparation has stage/operator cancellation; pixel/guide loops poll.
Codec stage cancellation, noncancellable compound export and uncompressed
PNG/native raster payloads remain. The public-mode gate and Phase 6 quality
gate stay unchanged.

## 7D assignment (completed within the supported internal subset)

Migrate geometry, paint and retouch in explicit batches through existing shared
algorithms, preserving the typed source/profile and old byte evaluation:

1. Add persistent u16 masks/channels, raw/effective coverage, independent extents,
   links, density and feather; version native records and shared capacity plans.
   Disclose the 256-level information of any promoted byte selection/brush input.
2. Generalize affine, perspective, resize/crop and mask sampling for typed
   sources. Keep float premultiplied intermediates; exact transforms preserve
   every source code and hidden RGB. Plan image/mask destinations before allocation.
3. Migrate painting, eraser, clone and fill through authoritative typed samples
   and colors. An RGBA8 overlay, preview, clipboard or CGImage cannot become the
   master source. Keep scalar tip-coverage limits explicit.
4. Migrate rasterized text/shapes and healing/retouch in bounded batches, with
   independent source/profile/alpha/writeback, resource, cancellation, capture
   and save/reopen checks for each supported target.
5. Keep unsupported operation/target combinations unavailable. 7E owns public
   mode, intentional conversion/downconversion, export dithering, and integration
   with existing central Undo, snapshots, recovery and UI/MCP/Actions guards.

## 7D result and historical handoff

[7D qualification](phase-7-geometry-paint-status.md) is complete: **974 Rust tests
pass**, with zero failures and two existing ignored in the normal suite. The
required opt-in million-unknown solver benchmark also passes with unchanged
accuracy. Authoritative Swift/app
checks, **13 Swift + 2 independent format groups**, **16 Release MCP checks**,
and all **21 native 24 MP workflow stages** pass. Native v13 preserves u16 masks
and channels, with v11/v12 reads retained. All existing byte evaluation remains
separate. The public mode and Phase 6 quality gates remain unchanged.

The 24 MP run took 25.945 s and reached **2.72 GiB RSS** under the unchanged
2 GiB tracked admission policy. Full masked projection took 0.388 s and full-source
bilinear affine 0.537 s. Paint/erase/clone stages include full-canvas Swift-Onone
coverage construction; local healing's 8.049 s includes two preparatory paints.
These are bounded workflow measurements, not an interactive-latency or arbitrary
multilayer/history qualification. Final tracked resources are zero.

**7D handoff: 7E** (now completed below), following the concrete [editor-mode handoff](phase-7-geometry-paint-status.md#limits-and-next-assignment-7e).
Integrate existing central document/history/recovery ownership and one shared
capability matrix before exposing mode selection, intentional conversion and
export dithering. Keep unqualified operations unavailable; do not introduce a
second Undo system or use byte display/clipboard pixels as typed masters.


## 7E result: public document mode

The [7E status](phase-7-public-mode-status.md) records completed public ownership,
supported editing controls, deliberate flattened conversion, export dithering and
the existing history/snapshot/recovery paths. A captured authoritative projection
feeds PNG/TIFF16 without a duplicate render; actual window close releases display
providers that AppKit's retained views would otherwise keep alive. The shared Undo
helper preserves AppKit event grouping across modal edits and actual Save.

Final qualification passes **999 Rust tests** (0 failures, 2 existing ignored),
formatting/Clippy, authoritative Swift typecheck, Release app build,
**18 Swift + 2 independent codec groups**, and **25 built-app MCP groups**.
All **nine public 24 MP workflow stages** pass at 6000×4000 with the original
window/history/projection retained. The fresh process takes **6.944 s**, peaks at
**2.25 GiB RSS**, and ends with all tracked resources zero. The unchanged 2 GiB
admission policy is not an OS RSS ceiling. This qualifies the declared workflow,
not arbitrary large multilayer/history combinations.

The [public support matrix](phase-7-operation-support.md) lists explicit refusals.
The Phase 6 cutout gate remains open. The 7E handoff originally named **8A**,
retained ordinary image sources. Phases 8A–8C now qualify embedded/linked ordinary
sources and retained RAW, verifying P7-01, P7-02 and P7-35.
The [8C result](phase-8-retained-source-status.md#phase-8c-qualification) preserves
original camera data, the effective recipe and captured decoder identity alongside
a genuine normalized sRGB u16 master and the document-depth fallback. Phase 8D now
adds explicit normalized sRGB/Display P3 developed sources, native detail and
complete source histogram/clipping inspection, Before/After comparison, presets,
settings synchronization and reviewed decoder migration. These additions pass
host/UI, external MCP and independent camera qualification.
Phase 8D is complete within its qualified scope; its final full Rust regression suite passes.
Phase 8E now qualifies retained native artwork, continuous attributed colors
and disclosed intrinsic glyph precision, verifying P7-04/P7-05. Full editing,
geometry/profile, UI/MCP/Actions, native and recovery evidence is recorded in
[the artwork status](phase-8e-artwork-status.md). Phase 8F qualifies retained host
stills and rational frame re-editing, with exact actual source samples/ICC/alpha,
originals and fallback persistence; see [host-source evidence](phase-8f-host-source-status.md#qualification).
Phase 8G qualifies camera-relative baseline compensation, supported moiré reduction,
calibrated manual vignette and ordered native source-local masks with alternate
exposure/WB camera develops, transfer and same-geometry migration; see [8G qualification](phase-8g-raw-enhancements-status.md#qualification).
### Historical handoff through Phase 9A

The following handoff records the status at the 9A milestone; its pending work
and carryover counts are superseded by the current update above.

**Phase 8 and the bounded Phase 9A source-Gaussian workflow are complete within their qualified scope. Phase 9B is next and has not started.**

Phase 9A adds one binding-owned live Gaussian in both document depths, with an independent native u16 source mask, exact prepared UI/MCP/Actions publication, native/snapshot/recovery recall and bounded cache/admission. Its [live-filter qualification](phase-9-live-filters-status.md#qualification) keeps source-profile conversion before filtering and accumulated affine/layer masking afterward. General selected destructive filters and scalar editing remain 9F/9G; multiple filters, sharpening/High Pass and presets follow in 9B. Six Phase 7 rows are verified and 30 remain open.

[Phase 8](phase-8-retained-source-status.md) records the handoff and limits.

These additions do not change historical Phase 7 or 8C measurements. P7-01,
P7-02, P7-03, P7-04, P7-05 and P7-35 are verified; the other 30 remain open,
and P7-07's selection/backdrop analysis stays in 9F. Source-local u16 coverage does
not close document selection/scalar-tool work in 9F/9G. Invalid preset diagnosis/removal/repair/versioning
belongs to 12B, with manual invalid-file removal the current recovery while normal
Develop stays usable. The typed RAW/host-media capture-packet/PPI bridge remains
in 11D, typed auxiliary mattes in 9G, and broader large-document/camera/codec
guarantees in 13D. Expanded ProRes cross-decoder conformance remains unqualified
in 11D/13D; 8F preserves every actual SDK sample exactly. Retained-source
group/perspective/whole-document geometry and general local-mask remapping remain in 12F.
The declared camera/decoder matrix and local mask/variant/request limits remain
explicit; broader camera/scale guarantees belong to 13D, individual codec
cancellation to 13C, and linear/HDR/float policies to 11E/11F.
