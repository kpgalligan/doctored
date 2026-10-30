# Phase 7E — public document mode

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

Status: **complete within the qualified public workflow**, following Phase 7D
(`7e60190`). The final checks below qualify the new editor paths; remaining
unsupported tools stay explicitly unavailable.

Subsequent source work is tracked in [Phase 8](phase-8-retained-source-status.md).
That record and the [current operation matrix](phase-7-operation-support.md)
describe the added embedded/linked sources, retained RAW with explicit cross-depth
fallback conversion, the 8D Develop workflow, retained artwork and qualified host/frame sources. The measurements below remain
the Phase 7E baseline.

Phase 7E connects typed storage to the existing `RasterDocument` owner,
central mutation, editing state, Undo/history, captured save, snapshots and recovery.
Typed owners never contain a byte surrogate. Authoritative u16 projections remain
separate from explicit display/interchange conversions.

Conversion is an explicit, undoable flattening operation. Byte-to-16-bit conversion
first evaluates the legacy composite, then promotes its samples exactly. Conversion
back to 8-bit evaluates the typed composite and applies the chosen quantization.
Both lose layer/mask/channel editability; the original remains in the same history.

## Implemented behavior

- `RasterDocument` owns exactly one byte or typed core document. Existing central
  editing state, Undo/history, saved identity, snapshots and recovery retain that
  owner. Typed projection and display conversion have separate storage and APIs.
- Preserving PNG/TIFF16 and typed native opening use the existing worker. Native
  signatures take precedence over filename extensions. Unsupported preserving
  inputs fail without invoking the byte ImageIO fallback or working-space adoption.
- Image > Mode and MCP `get_precision` / `convert_precision` expose precision and
  an explicit flattened conversion with a before/after preview. Conversion resets
  discarded plane targets as part of the same Undo transition. Actions retain the
  depth choice and exact authored document-space colors.
- A shared operation/target/depth gate covers UI, MCP and Actions, with core
  capability and parameter/resource checks. The [support matrix](phase-7-operation-support.md)
  lists enabled tools and refusals. Legacy byte APIs never receive a typed pointer.
- Tone/profile controls and numeric affine/perspective controls prepare typed
  previews. Brush, erase, clone, healing, mask operations and raster text/shapes
  use the qualified typed core paths. AppKit supplies only explicit scalar drawing
  coverage; color and source pixels stay typed.
- Native sample sums/means and histogram endpoint counts use authoritative u16
  pixels. The 256 displayed histogram bins do not stand in for exact clipping.
- Native master saves preserve editable typed state. Captured exports offer
  PNG/TIFF16 or deliberate 8-bit output with none/ordered RGB dithering, undithered
  alpha and a loss report. PNG/TIFF16 uses the captured current u16 projection,
  avoiding a second full-frame projection. Core admission checks the document/image
  union, exact dimensions/profile and unchanged codec scratch. Export does not
  replace or mark a newer master saved.
- Public editing-state archive v2 carries storage/evaluation/export policy and
  retains v1 byte compatibility. Recovery includes named snapshots. Facade
  metadata retention and decode peaks are included in archive/resource admission.
- Typed edit preparation finishes before mutation/history publication. Worker
  results bind document identity, revision, target and selection; stale work
  refuses. History publication excludes reentrant mutations and nested Undo/Redo.
  If another captured worker temporarily owns the queue, history display refresh
  waits and authoritative projection stays unavailable until it is current.
- Actual window close clears and redraws cached display images before AppKit can
  retain the closed views. Document/history and captured worker inputs keep their
  independent owners; cancelling a close does not clear the visible image.

The numerical conversion/dither/readout contracts and cancellation boundaries are
recorded in [conversion and readout](phase-7-conversion-readout.md). Native format
versions stay v11–13 for typed documents and legacy v1–10 retain their evaluation.
Public archive v2 is a host manifest change, not a reinterpretation of byte pixels.

## Qualification

All checks below use the final sources, including captured-projection export,
Undo event grouping and window-display teardown:

- **999 Rust tests passed, 0 failed, 2 existing ignored**, across 94 result groups.
  Formatting and all-target Clippy pass. The final four projection-export tests
  check every u16 code, independent PNG/TIFF decoding, ICC/PPI/XMP, strict refusals,
  aliased input accounting, exact admission boundaries and atomic failure.
  Evidence: `/tmp/rz-phase7e-projection-core-summary.json` and
  `/tmp/rz-phase7e-projection-core-tests.log`. The normal archive and **274 core
  source/test/header/dependency hashes** are recorded in
  `/tmp/rz-phase7e-projection-staticlib.json` and
  `/tmp/rz-phase7e-projection-source-snapshot.json`.
- Authoritative **`make typecheck`: 0 errors**, plus a successful Release
  **`make app`**. Logs: `/tmp/rz-phase7e-typecheck-projection-frozen.log` and
  `/tmp/rz-phase7e-app-build-projection-final.log`. All **293 app/header/project
  hashes** match `/tmp/rz-phase7e-projection-app-source-snapshot.json`.
- **25 built-app MCP groups pass: 16 legacy and 9 typed**, with a 142-tool
  catalog. This includes exact source-first clone, native sampling, authored
  colors, typed edits/groups/masks/channels, Actions, portable snapshots,
  independently decoded PNG8 none/ordered output, cross-depth Undo/Redo and
  recovery after a controlled crash/restart. Report:
  `/tmp/rz-phase7e-mcp-report.json`; log: `/tmp/rz-phase7e-mcp-smoke.log`;
  artifacts: `/tmp/rz-phase7e-mcp-2d7oanw1`.
- **18 Swift host groups + 2 independent Python format groups pass**. These
  cover typed ownership/display boundaries, conversion/history, archives and
  recovery, stale document/target refusal, captured-old/current projection
  exports, mixed resource unions and full release. Both byte and typed documents
  pass actual Save success/error and Undo/Redo checks with default event grouping,
  and actual close-button checks release their pixel owners within two seconds.
  Report: `/tmp/rz-phase7e-host-small-report.json`; log:
  `/tmp/rz-phase7e-host-small-projection-final.log`; artifacts:
  `$TMPDIR/rasterize-precision-probe-wge1j6sm`.
  All **302 app/header/probe hashes** match. This functional run overlapped app
  compilation; its 6.39 s / 474,284,032-byte peak RSS is diagnostic, not an isolated
  performance measurement. No diagnostic interposer was used.

The source probe privately copied the qualified normal archive before Xcode
rebuilt its own archive from the same frozen core sources. The app and probe
therefore have separate binary provenance:

| Artifact | SHA-256 |
| --- | --- |
| Qualified normal Rust archive / host probe dependency | `291441f28359901748a905f06dc5f011d9327a5703de303c17da11ae2ed1e168` |
| App-produced Rust archive | `d42346ef007f4e5407403cc70a61c54b15f9b9c9665011c7c3f9c046e2815735` |
| Final Release app executable | `ea808fe068d60bb3118f23b9b027790cfd667c9097301d34e388bda4b35a2c65` |
| Final source-probe executable | `7f6385d6081fc90ed1ccca54cddeec853cd7164e7768b1b8ce8ffccef977a60b` |

App provenance: `/tmp/rz-phase7e-projection-app-build-provenance.json`.
Private host-library copy: `/tmp/rz-phase7e-final-host-library.json`.
Complete host artifact/source manifest: `/tmp/rz-phase7e-final-host-artifacts.json`.
The MCP report separately records its ad-hoc-signed private bundle hash.
Every owned test process exited. Tests used dummy credentials and isolated
preferences, Actions, recovery and ports; existing user app instances were untouched.

## Public 24 MP result

**All nine stages pass** in a fresh process at **6000 × 4000**, with the unchanged
**2 GiB admission policy** and no resized substitute. The real public window,
current projection/display, saved owner and one Undo transition remain retained
through editing, persistence and export. The temporary reopened document is closed
and allowed normal AppKit event turns to release before the next admission.

| Stage | Seconds |
| --- | ---: |
| Preserving public open | 1.304 |
| Captured typed paint, including full-size Swift coverage construction | 3.552 |
| Undo | 0.342 |
| Redo | 0.340 |
| Actual native Save | 0.233 |
| Native reopen, verification and temporary-document release | 0.438 |
| PNG16 export from the captured authoritative projection | 0.160 |
| Preserving PNG16 reopen and verification | 0.297 |
| Close public window and release | 0.056 |

The process took **6.944 s**, with **2,412,609,536 bytes peak RSS (2.25 GiB)** and
2,368,309,960 bytes peak memory footprint. Swift is `-Onone`, Rust is Release;
this is a bounded workflow measurement, not an optimized interactive-latency claim.
The 2 GiB tracked admission policy is **not an OS RSS ceiling**.

Actual export admission was **674,117,788 retained + 1,287,362,304 scratch =
1,961,480,092 bytes**, below 2,147,483,648. The codec estimate was not reduced and
history/projection were not dropped. Independent paint references, 257 distributed
source samples plus painted samples, exact alpha/profile and exported RGB checks
pass. Temporary native-close settling took 0.012 s; final close settling took
0.053 s, both inside the fixed two-second bound. **All tracked handles, capture
payloads, retained bytes and scratch are zero after close.**

Evidence: `/tmp/rz-phase7e-host-public-qualification.json` and
`/tmp/rz-phase7e-host-public-measurement.log`; the probe artifact directory above
contains `files/large-public-report.json` and all full-resolution outputs.
The report rejects missing resource data, refused stages or unreleased owners.

Reproduce the host checks after building the current Release library with
`python3 -B scripts/precision-probe/verify.py --keep --large-public`.
Run the large branch after other builds/app probes finish; its
[runner notes](../scripts/precision-probe/README.md) describe the retained scope,
isolation and numerical contracts. Earlier internal 7B–7D large measurements do
not substitute for this public-window/history measurement.

## Corrections established by qualification

The real Save regression exposed a shared Undo-helper defect: a modal-timer edit
created and manually drained an implicit event group, leaving AppKit expecting it
at Save completion. `withUndoGroup` now disables automatic grouping only before
opening a group at level zero, closes its own group and restores the policy.
Existing caller groups are preserved. The enhanced byte/typed Save and Actions
checks pass with this fix. Trace: `/tmp/rz-phase7e-undo-fulltrace-t9e8i9r2/run.log`.

A standalone AppKit reproduction established that closed views could retain the
CGImage provider and its full pixel buffer. Explicit display teardown on actual
window close fixes that lifetime. The first large run also bypassed normal
file-type/URL initialization; the final fixture uses the same document-controller
open path as public MCP. The old compound export plan exceeded the public retained
scope; reusing the already-authoritative u16 projection removes that duplicate
allocation while preserving codec estimates. Earlier failed attempts and the
pre-extension 995-test evidence are retained and do not stand in for the final run.

## Supported boundaries and next assignment

The [support matrix](phase-7-operation-support.md) is the current public contract.
Every remaining feature/qualification gap is assigned to a named later slice in
the [carryover ledger](phase-7-carryovers.md), with explicit acceptance criteria.
Those open assignments are required work for Phases 8–13; Phase 7 completion does
not close them.
Numeric affine/perspective controls are supported; Canvas Size is supported at
every depth since [12F-1](phase-12f-geometry-status.md), and STRAIGHTENING (a
`crop` with a non-zero `angle`, and the Crop tool itself) at 16 bits since
[12F-1b2](phase-12f-geometry-status.md); drag transforms and canvas
Move are unavailable. Canvas brush coverage uses
fixed size without pressure/airbrush. Selected destructive tone, selection-gated
histograms, color-match bucket fill, u16-channel-to-byte-selection reduction,
scalar-plane painting and clipboard editing remain unavailable. Styles,
other blends, content-aware tools, retained text/shape descriptions and high-depth
PSD are separate work. [Phase 8C](phase-8-retained-source-status.md#phase-8c-qualification)
now qualifies retained high-depth RAW within its declared camera/decoder scope: original
RAW, effective recipe and genuine normalized sRGB u16 master survive redevelopment,
retouch and native/recovery reopen. The byte mode retains that master behind its byte
fallback. Saved rendering remains usable when the captured decoder is unavailable.
Phase 8D adds an explicit normalized sRGB/Display P3 source choice, native detail
inspection, complete source histogram/clipping, Before/After comparison, presets
and settings synchronization. Explicit decoder migration reviews the saved
rendering against a prepared candidate and accepts it through central Undo and
captured Actions replay. These additions pass host/UI, external MCP and independent
camera qualification. P7-07's selection-gated/partial-backdrop analysis stays in 9F.
Typed RAW imports retain camera metadata inside the original file but report empty
export packets/default PPI; the typed import bridge belongs to 11D. Camera auxiliary
mattes are not extracted into typed channels until 9G. Phase 8G qualifies camera-relative
baseline compensation, supported moiré reduction, calibrated manual vignette and
ordered source-local u16 masks with alternate exposure/WB camera develops. Exact
masks/effective recipes survive native/recovery, transfer and same-geometry migration;
see [8G qualification](phase-8g-raw-enhancements-status.md#qualification). Invalid preset diagnosis/removal/repair/versioning
belongs to 12B; normal Develop remains usable when an invalid preset blocks library
operations, with manual file removal the current repair. Ordinary canvas brush/selection coverage retains 256 levels; Phase 8E generated artwork uses
native area coverage with separate continuous ink and disclosed intrinsic-byte
glyph assets. Source RGB/alpha and stored masks/channels retain u16 precision.

This is normalized encoded SDR, with no HDR or linear-light policy. The 24 MP
qualification covers the declared single-edit workflow, not arbitrary large
multilayer/history combinations or every export format at that size. Codec calls
remain bounded but do not promise live cancellation. Phase 13 remains necessary
for broader large-document guarantees.

### Historical handoff through Phase 9A

The following handoff records the status at the 9A milestone; its pending work
and carryover counts are superseded by the current update above.

The **Phase 6 cutout quality gate remains open**; its completion is assigned to
9I without changing the original criteria. Its experimental controls remain
unavailable. The 7E handoff originally named **8A: retained ordinary image sources**;
8A–8C are now complete within their qualified scope, verifying P7-01, P7-02 and P7-35.
Phase 8D is complete within its qualified scope; its final full Rust regression
suite passes. Phase 8E now qualifies retained native text/shapes, continuous
attributed colors and intrinsic glyph assets through UI/MCP/Actions, profile/
geometry edits, native and recovery, verifying P7-04/P7-05. Phase 8F verifies P7-03
with retained full host stills and rational movie frames, exact original/master/ICC/alpha
and native/snapshot/crash-recovery re-edit in both fallback depths. Six rows are
verified and 30 remain open. Phase 8G completes the bounded RAW enhancements.
**Phase 8 and the bounded Phase 9A source-Gaussian workflow are complete within their qualified scope. Phase 9B is next and has not started.**

Phase 9A adds one binding-owned live Gaussian in both document depths, with an independent native u16 source mask, exact prepared UI/MCP/Actions publication, native/snapshot/recovery recall and bounded cache/admission. Its [live-filter qualification](phase-9-live-filters-status.md#qualification) keeps source-profile conversion before filtering and accumulated affine/layer masking afterward. General selected destructive filters and scalar editing remain 9F/9G; multiple filters, sharpening/High Pass and presets follow in 9B. Six Phase 7 rows are verified and 30 remain open.

current source progress and limits are tracked in [Phase 8](phase-8-retained-source-status.md).
Broader camera/codec-corpus qualification and the failed expanded ProRes cross-decoder comparison remain in 11D/13D; typed host export packets/PPI and auxiliary mattes remain in 11D/9G. Exact actual SDK-buffer preservation is qualified separately. Invalid preset recovery remains in 12B; retained-source group/perspective/whole-document geometry and general local-mask remapping remain in 12F. Local-mask caps and file-specific camera qualification remain explicit; larger camera/history/variant workloads belong to 13D, individual codec cancellation to 13C and linear/HDR/float policies to 11E/11F. The 8D results do not change
any historical Phase 7 or 8C measurement.
