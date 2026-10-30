# Phase 3 — TIFF, color and capture metadata

Implementation record for the [professional photo editor plan](professional-photo-editor-implementation-plan.md), September 9, 2026. **Phase 3 (3A–3D) is implemented and passes its completion gate within the scope below.** Phases 1 and 2 are committed in `95a8397`.

## Delivery slices

| Slice | Implementation | Verification |
| --- | --- | --- |
| 3A — TIFF round trips | Typed TIFF writer, embedded ICC on input, actual document PPI, upright orientation and straight alpha declaration | Core tests and independent libtiff/ImageMagick/ImageIO reads pass, including built-app round trips |
| 3B — Capture metadata | Bounded standard TIFF EXIF/GPS/Interop relocation, TIFF XMP/IPTC, ImageIO standard RAW/HEIC capture fields and XMP, explicit omission reports | Core tests, actual CFA RAW/HEIC imports, native packets and independent export reads pass |
| 3C — Calibrated operations | Shared document-profile Exposure/White Balance, explicit persisted evaluation, legacy descriptions and Actions retained | Independent color oracles, native migration, real sheets, MCP/Actions and scalar/profile refusal checks pass |
| 3D — PSD compatibility | Compatibility report, explicit layers/composite choice, color/depth refusal, source fingerprint | Core tests, pre-edit AppKit notices, MCP choices/fingerprints and refusal workflows pass |

## Scope and contracts

TIFF remains RGBA8 in this phase, with classic TIFF metadata and the first image supported. BigTIFF metadata and additional pages are outside this slice. The direct typed TIFF encoder can accept higher-depth storage when Phase 7 provides it; this change does not introduce a high-depth document mode. TIFF resolution is written as inch-based rational values derived from the document. On import, orientation is baked into pixels and quarter turns exchange the resolution axes.

TIFF EXIF offsets refer to the whole source file. Standard capture fields are relocated into a compact packet, excluding pixel layout and preview data. Unknown private fields and MakerNotes are reported as omissions. TIFF export refuses an EXIF packet as a whole when safe relocation would omit unsupported fields; existing save/export loss reporting reflects that refusal. Supported XMP and IPTC packets keep the existing native packet caps.

The ImageIO RAW/HEIC path reconstructs standard EXIF/GPS, TIFF descriptions and IPTC fields through a tiny TIFF metadata carrier and serializes ImageIO's XMP view separately. Carrier pixels/profile/PPI are never adopted by the photograph. If ImageIO omits valid density tags from a CFA DNG, the core scanner supplies upright PPI without decoding the sensor mosaic. Original packet bytes, MakerNotes, private camera dictionaries and decoder-specific metadata are outside this guarantee. Import reports distinguish supported scope from known omissions; a source that was not inspected retains `metadata_not_captured`.

New RGB Exposure and White Balance edits explicitly store `params.evaluation = "document_v2"`. Evaluation uses the document's supported RGB matrix/TRC profile through shared kernels and the existing ICC forward-threshold encoder. Missing evaluation or `legacy_srgb` retains the previous numeric processing. Scalar targets retain profile-independent grayscale semantics. Unsupported profiles refuse calibrated operations; a saved calibrated layer with an unsupported profile produces an explicit render failure while native saving remains available. Native writer version 9 records the new evaluation contract without changing the container layout; versions 1–8 remain readable.

PSD reports distinguish general importer limits, observations and verified per-file losses. The app presents available choices before editing; MCP preflight supplies a source fingerprint and the open names the chosen mode. Import supports 8-bit RGB/grayscale; stored composites support raw and PackBits compression, with ZIP composites refused explicitly. Detailed import of discarded Photoshop records remains Phase 10 work.

## Verification record

The full optimized Rust suite (`CARGO_BUILD_JOBS=4 cargo test --release --no-fail-fast`) passed: **793 passed, 0 failed, 1 ignored**, across 59 result groups. The ignored test is the existing optional million-unknown Poisson accuracy check; that solver was unchanged. Final `make lint` and `cargo fmt --check` passed.

The final isolated app build and all three Phase 3 MCP suites passed, including the RAW density fallback. All seven Phase 1/2 MCP regression suites passed against the new app. Final `make typecheck` passed with 0 errors and 6 existing warnings. All 11 AppKit source-probe groups passed.

- `core/tests/metadata_tiff_tests.rs`: independently assembled little/big-endian TIFF, relocated EXIF/GPS/Interop, large metadata offsets, packet limits, malformed input, straight alpha, libtiff/ImageMagick reads, and metadata-only RAW density fallback.
- `core/tests/metadata_import.rs`: platform carrier attachment without pixel/profile/PPI changes, no-op semantics, omission reporting, packet bounds and NULL handling.
- `core/tests/calibrated_adjustment_tests.rs`: independent profile matrices/transfer curves, equivalent colors, wide-gamut output, legacy/native evaluation and unsupported profiles.
- `core/tests/psd_import_notice.rs`: raw/PackBits saved composites, merged alpha versus saved channels, grayscale, masks, source fingerprints, unsupported depth/color and usable composites behind broken layer records.
- `scripts/color-metadata-probe/`: actual ImageIO TIFF/HEIC output, genuine synthetic CFA RAW development, independent export reads, packet limits, live adjustment-sheet previews/Apply, selection and Actions parity, PSD notices before document publication, Revert and Batch refusal.
- `scripts/verify_phase3.py`: built-app MCP capture/300-PPI export/native/strip flows; calibrated destructive/layer/Actions equivalence; PSD preflight, explicit mode and stale/unsupported input refusal.

Verification uses an Xcode Debug app with the release Rust core and Swift `-Onone`, whole-module compilation. Optimized Swift output is not qualified by this phase. The app is built at `/tmp/rz-phase3-build`; the final source-probe artifacts are retained at `$TMPDIR/rasterize-color-probe-0gv2nibk`. Runners use private ports/settings, dummy credentials, temporary fixtures and only processes they launch. HEIC fixture generation requires macOS ImageIO/IOSurface services; it was verified outside the filesystem sandbox. The synthetic DNG checks RAW decoder/metadata plumbing, not camera rendering quality or high-depth editing. Temporary logs and build outputs stay outside the repository; retained verification scripts live under `scripts/`.
