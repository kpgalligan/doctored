# Phase 8F — retained host image and frame sources

## Current state

Implemented after the separate 8E commit `23cf25d`. **8F is complete within the qualified scope below, and P7-03 is verified.** Six Phase 7 assignments are verified and 30 remain open. **8G: camera controls and local RAW development is next.** This file records the current work and evidence; the [shared source status](phase-8-retained-source-status.md) and [implementation plan](professional-photo-editor-implementation-plan.md#8-retained-sources-and-revisitable-raw) retain the phase handoffs.

The implementation combines immutable encoded-resource/native persistence and exact host-buffer packing in Rust, host still/video decoding with captured UI/MCP/Actions edits, and independent fixture/native-payload/actual-app qualification. All required 8F preservation and workflow gates pass; the separate failed expanded cross-decoder comparison remains explicitly open below.

## Bounded contract

- Retain exact encoded originals, actual decoded master samples and ICC independently from the document's rendered fallback depth. A genuinely byte source remains a byte master in a 16-bit document.
- Save a strict host decoder/codec contract and exact rational requested/resolved frame identity. Compatible re-edit uses captured originals; unavailable or changed decoders retain saved rendering and refuse a new decode. A selected still never silently falls through to a movie frame.
- Embed one still/movie resource or a two-resource Live Photo pair in a capped, shared binary resource table. Native/history/recovery reopen needs no platform decoder. The generic resource framing must also support the separately implemented 8G mask attachments.
- Route publication through the existing single worker, captured document/target/selection checks and central Undo. Presence guards must not depend on an admitted metadata read; failed descriptor restoration must never publish a raster-only result.

The initial preserving codec corpus is lossless Display P3 JPEG XL, HEIC 8/10-bit, SDR ProRes 4444 with alpha, and SDR H.264 byte frames, with paired JPEG/HEIC stills. Actual full decoder output determines storage/layout/profile; container depth and requested buffer precision are recorded separately from evidence of preserved samples. Broader formats, grayscale/metadata/PPI import and HDR/float behavior remain with 11D/11E/11F. General animation composition/disposal is not implied by ImageIO's image-count API.

## Implemented source workflow

Opening JPEG XL/HEIC, a supported movie or a same-basename JPEG/HEIC+movie pair captures private copies before decoding. A pair starts with its full still; filename pairing is disclosed and does not claim an Apple asset-ID match. The full ImageIO/AVFoundation buffer is unpacked by the core using its declared integer channel order, endianness, alpha mode, row padding and exact orientation. Unsupported HDR/float/profile/layout or an unverified high-depth-to-byte decode refuses. The working document can use either fallback depth while the actual master remains 8-bit or genuine u16 as supplied by the decoder.

The frame sheet separates Original Still from Movie Frame. Movie requests select the greatest presentation timestamp at or before the request inside the track's start-inclusive/end-exclusive interval. Exact `value/scale` input stays rational; decimal user seconds explicitly round to microseconds and the accepted request is shown beside the resolved timestamp. Missing SDK sample durations are labelled as coverage to the next PTS or track end. The native source dimensions/profile can change between still and movie; a separate fit keeps the source canvas stable, and one final binding composes that fit with accumulated user affine. Independent masks, retouch and properties remain separate. Preview Frame prepares the entire candidate; Apply publishes those same pixels, with no second decode. Same resolved frames add no Undo.

A placed host source lands at the slot the `position` / `layer` pair names — above the anchor by default, at the top of the root level, or as a group's top child — at both depths since [12E-2b](phase-12e-layer-ops-status.md), which also made `add_live_photo_layer`'s legacy `layer` field the placement ANCHOR and deleted its "retained media placement appends a top root" refusal; the attachment is written at that LANDING index rather than at `layerCount - 1`, and every attachment writer now answers to the layer's PIXEL lock. `get_host_source` exposes the contract, original-resource identities and saved-rendering/decoder status. `set_host_frame` shares the captured UI publication path; the existing Live Photo tool names forward to it. Actions retain exact frame identity, original hashes and decoder version. Legacy path-based descriptions display their cached pixels unchanged on native open; an explicit edit captures their exact available files, preserving the old description in Undo. Missing/changed/unsupported legacy originals refuse without discarding saved work.

## Persistence and resource boundaries

Native byte version **21** and typed version **22** add a common deduplicated binary resource pool. Both preserve actual source masters and rendered fallbacks; typed 22 also carries the existing artwork table. Older readers/writers remain selected when the new attachments are absent. Original bytes can share allocations across frame revisions and with retained RAW assets. Native/snapshot/recovery reopen never invokes a host decoder. Before re-edit, the host checks the original hashes and the saved master dimensions/depth/ICC against its descriptor; decoder/format/profile changes refuse rather than reinterpret the source.

Each host source contains one original or one ordered still/movie pair. The generic framing permits four resources for the separately planned 8G attachments; each resource is capped at 256 MiB and the document's unique combined RAW/attachment originals at 512 MiB. The envelope is at most 64 KiB with 48 KiB opaque state. Pixel/dimension, metadata, reference and aggregate bounds are checked before core pixel allocation. The host admits private encoded copies and decoded buffers through the shared registry; this accounting is not a process-RSS guarantee. Movie policy is one stable track/format, at most 120 seconds and 4,096 samples. AVC compressed inspection additionally caps packets, NALs, SEI and empty stream-control buffers. Individual ImageIO/codec calls are noncancellable; sample/NAL loops and subsequent core stages cooperate between bounded steps.

Byte still imports retain the existing capture-packet/PPI and auxiliary-matte pass. Typed host-source and selected-movie imports preserve all metadata inside the embedded originals, but explicitly report no reconstructed export packets, original PPI or auxiliary channels. The typed packet/PPI bridge belongs to **11D**, matte/channel import to **9G**, HDR/float to **11E/11F**, and broader codec/size/cancellation qualification to **11D/13C/13D**. Local RAW controls/masks remain the separate **8G** assignment; retained-source group/perspective/whole-document geometry still has the explicit rasterization boundary owned by **12F-2**, with ONE exception already lifted by [12F-1](phase-12f-geometry-status.md): Canvas Size reads no sample, so a retained source rides it at both depths and keeps its master.

## Qualification

The preserved preimplementation feasibility corpus is `/tmp/rz-phase8f-feasibility/manifest.json` (36 hashed artifacts), with independent authored arrays, platform decoder buffers and primary-reference notes. These standalone probes establish feasible layouts, not final application qualification.

**The corpus is GONE on this machine, so `scripts/verify_phase8f.py` is NOT RUN — owner 13D.** It reads `/tmp/rz-phase8f-fixtures`, which `scripts/host-source-probe/fixtures.py` builds from that preserved `/tmp/rz-phase8f-feasibility`, and both are absent (`/tmp` does not survive the machine); 12E-2b recorded the same miss rather than fabricating a corpus. 13D owns scale/corpus qualification. **Accepted when:** the 8F fixture corpus is re-authored by a COMMITTED script into `~/devel/rasterize-fixtures/phase8f/` (never `/tmp`), `verify_phase8f.py` reads it from there, and the drive passes.

The fixed gates include exact authored JXL RGBA16/ICC; exact platform decoded buffer through core packing, saved master and native/recovery cycles; the unchanged **single ProRes calibration fixture** bounded at 64 u16 codes per RGB component against independent BT.709 reconstruction and exact authored alpha expansion; byte-source controls; distinct rational-time frames; and explicit unsupported HDR/layout/codec refusals. Actual UI/MCP/Actions/history, source deletion, stale targets, admission and decoder-change cases run in both fallback depths using private stores, dummy credentials and owned test processes.

The independently expanded multiframe ProRes cross-decoder comparison failed that 64-code bound before product testing (a frame reached 79 codes). Further characterization separated differences in the two native YUV decoders from AVFoundation's own RGB conversion. **Expanded cross-decoder conformance remains unqualified**, assigned to 11D/13D; the bound was not widened. This is distinct from the mandatory universal 8F gate: every selected frame must preserve its independently captured actual AVFoundation samples, ICC, alpha and rational timing exactly through the application and native/recovery persistence. The phase makes no claim of cross-decoder bit equality or certified encoded 12-bit codec accuracy. Evidence: `/tmp/rz-phase8f-prores-decoder-characterization.json`.

The final numerical and workflow results are:

| Gate | Final evidence |
| --- | --- |
| Core regression and resources | **1,123 passed, 0 failed, 2 existing ignored**; formatting and all-target Clippy pass. The new packing/native tests cover all accepted layouts, alpha rounding, all eight orientations, preallocation refusal, cancellation, shared originals, mixed RAW/host/artwork persistence, malformed refs and old-format compatibility. |
| Authored still reference | Display P3 JXL preserves all **147,456 authored RGBA16 components and ICC exactly**, including hidden RGB; 146,887 components are off the byte grid and alpha has 36,769 values. HEIC10 full output has 110,107 non-byte components; HEIC8/JPEG and H.264 controls remain actual byte sources. |
| ProRes calibration | The unchanged single calibration case has RGB maxima **52/27/60 u16 codes**, within the declared 64-code bound; authored 10-bit alpha expansion is exact with 1,024 levels. Expanded multiframe cross-decoder conformance remains failed/unqualified as described above. |
| Host source/native comparison | **56 saved masters**, **10,911,744 source components** exact, with exact ICC, original SHA, orientation and rational timing/duration provenance. |
| Independent identity fallback | **589,824 fallback components** exact against independent native conversion for JXL16 and HEIC8 in both document depths, including latent RGB and document ICC. Persistence/display hashes alone were not used as the conversion oracle. |
| Actual compiled host workflow | **39 groups pass:** 19 direct-media/AVC/legacy, 2 denied-admission and 18 actual frame-sheet groups. Preview/Apply identity, every preview byte/ICC, Undo/Redo, no-ops, stale source/selection, Cancel and actual close release pass. Accounting-only leases prove cheap presence and atomic frame refusal without large allocations. |
| External Release app | **26 groups, 536 traced calls** pass in both depths with **9,437,184 exact source components**. Coverage includes independent masks/affine/retouch, Actions, strict arguments/forged descriptors, unknown decoder policy, corrupt original hashes, snapshots, fresh native opens and actual crash recovery after deleting both pair originals. |
| Build/provenance/cleanup | Authoritative zero-error typecheck and macOS-15 Release build pass. All **340 compiled host inputs**, **680 used host inputs**, **792 external-MCP inputs**, **49 fixture files** and **73 reference files** match their recorded hashes and file sets. Final host PID 33107 and all six owned external app PIDs exited. |

Runtime qualification found and fixed byte-native-21 dispatch, incomplete rational alias input and AVFoundation's valid attachment-only compressed stream buffers. The final descriptor audit also rejects contradictory derived `next_pts`/`track_end` intervals while retaining positive actual SDK `decoder_sample` durations, which can extend beyond displayed media because of trimming. The Cancel lifetime measurement uses an explicit probe autorelease pool and still requires both the complete candidate and NSImage to release while the sheet stays alive; no product lifetime gate was weakened. All final checks use the corrected source snapshot.

The actual owned frame-sheet captures fit within 620 × 561 points, with complete source/time/profile readouts and all buttons visible. The final byte and typed captures were independently visually reviewed. This corroborates the numeric UI checks; it is not an independent decoder-color oracle.

### Reproduction and final artifacts

The [probe instructions](../scripts/host-source-probe/README.md) describe fixture/reference generation and isolated reproduction. `scripts/verify_phase8f.py` snapshots source inputs and private test stores; the final host reuse run validates the unchanged compiled binary/archive/input set before rerunning all 39 groups. It does not treat a stale executable as a current-source build.

| Artifact | Location / identity |
| --- | --- |
| Core result and source audit | `/tmp/rz-phase8f-core-summary.json` |
| Final host report | `/tmp/rz-phase8f-host-final.json` |
| Final external MCP report | `/tmp/rz-phase8f-mcp-final.json` |
| Independent decoded/reference metrics | `/tmp/rz-phase8f-independent-numeric.json`, `/tmp/rz-phase8f-reference-provenance.json` |
| Independent final hashes and file-set audits | `/tmp/rz-phase8f-independent-final-audit.json`, `/tmp/rz-phase8f-final-audit.json` |
| Final host source/compiled manifests and captures | `$TMPDIR/rasterize-phase8f-workflow-z7qh6sgt/source-provenance.json`; its `host/` contains `compiled-provenance.json`, the private archive/binary and `results/ui/rgba8_frame-sheet.png` / `rgba16_frame-sheet.png` |
| Final typecheck / Release logs | `/tmp/rz-phase8f-typecheck-third-freeze.log`, `/tmp/rz-phase8f-app-third-freeze.log` |
| Core archive SHA-256 | `abac751b9c5cb04392837024215ab8443eefa351825d5728737560cfc92ad9a4` |
| Release app executable SHA-256 | `fad69a83db3c8bb2cd9ca4484efe15ff7b83df7172d9e6b9ac76a33256915873` |
| Compiled host executable SHA-256 | `542ffa54a8860cdd6d48ca5c8840028658018b5c4ca7d96a60ff43ce37b81e6a` |

The original 8E/7E counts and earlier exploratory reports remain historical evidence. Phase 8F does not change their workload claims or close the Phase 6 cutout gate. **8G remains required**, with typed metadata/mattes, broader codec conformance, HDR/float, source geometry and larger workloads assigned above rather than dropped.
