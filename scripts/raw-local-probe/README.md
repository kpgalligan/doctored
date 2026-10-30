# Phase 8G independent RAW-local qualification

The numerical gates are frozen in
[`phase-8g-raw-enhancements-status.md`](../../designs/phase-8g-raw-enhancements-status.md).
The root-owned `core/tests/raw_local_numeric.rs` uses analytical float64 coverage,
composition and calibrated sRGB/P3 vignette references. Bounds are one stored
mask code, `2e-6 + 0.5/65535` blend error and three stored vignette codes; neutral,
disabled, outside-mask and alpha cases remain exact.

`reference.swift` compiles alone against the macOS SDK. It renders complete
genuine camera frames into u16 output at explicit sRGB/P3 encodings; no app/core
decoder, image resampling or display preview supplies reference pixels. Camera
comparison stays within three u16 codes. The highlight gate compares a clipped
positive-exposure base with a genuine negative-offset RAW development, requiring
additional source detail that darkening the clipped output cannot recover.

Use the exact CC0 Nikon D70s fixture identified in
[`scripts/raw-probe/camera-fixture.json`](../raw-probe/camera-fixture.json).
The synthetic oriented DNG qualifies fast UI/state checks; it never substitutes
for the genuine 3008×2000 camera pipeline. Original camera files remain outside
the repository. Shared fixture/reference files are immutable during qualification.

All host/MCP runs use private snapshots, stores, ports and dummy credentials,
retain source/header/Cargo/build/helper hashes and exact binary/archive hashes,
and terminate only owned processes. Camera decoding needs actual macOS graphics
services; a denied/all-zero decode fails rather than becoming a reference.

Reproduce on the recorded macOS/decoder build using the downloaded CC0 camera
path (the generator rejects a different SHA):

```sh
python3 -B scripts/raw-local-probe/fixtures.py --camera /path/to/camera.NEF --output /tmp/rz-phase8g-fixtures
xcrun swiftc -O -target arm64-apple-macos15.0 scripts/raw-local-probe/reference.swift -o /tmp/rz-phase8g-reference
/tmp/rz-phase8g-reference /tmp/rz-phase8g-fixtures/reference-jobs.json /tmp/rz-phase8g-reference-output
MACOSX_DEPLOYMENT_TARGET=15.0 make app
python3 -B scripts/verify_phase8g.py --mode all --require-current --report /tmp/rz-phase8g-final.json
```

`oracle.swift` is compiled separately with Foundation only. It compares the actual
complete CI variant inputs with the standalone SDK frames within three codes,
then evaluates analytical masks and the ordered blend against those **exact
product inputs**. This keeps SDK rounding separate from the tighter native blend
gate. The final vignette oracle reads the actual source ICC transfer functions,
operates on the unquantized blend, and quantizes once. All four full-size camera
cases require exact outside-mask samples and opaque alpha. Named Apple reference
spaces and the app's equivalent built-in ICC encodings need not have identical
ICC file bytes; product source/profile persistence is checked byte for byte.

Phase 12F-4 additionally compiles `geometry-reference.swift` by itself with
Foundation only, comparing the saved native RZLM masks against an independent
f64 exact/affine sampler (one u16-code maximum). Host and MCP drives at both
fallback depths require five named registered-vignette migration groups:
`registered-vignette-crop`, `registered-vignette-pad`,
`registered-vignette-exif`, `registered-vignette-affine-chain`, and
`registered-vignette-ui-replay-reopen`. Each checks the saved reference frame,
reviewed candidate, replay and native reopen; the UI group drives an authored
vignette-center change on the migrated source. These synthetic DNG geometry
checks supplement, never replace, the pinned Nikon full-camera corpus.

The small standalone C contract check is independent of the Swift host:

```sh
xcrun clang -std=c11 -Wall -Wextra -Werror -target arm64-apple-macos15.0 -I core/include scripts/raw-local-probe/abi.c core/target/release/libdoctored_core.a -framework Security -framework CoreFoundation -framework SystemConfiguration -liconv -o /tmp/rz-phase8g-c-abi
/tmp/rz-phase8g-c-abi
```

The runner retains immutable compiled sources, build/run logs, native files,
exact camera inputs, masks, UI screenshots, numeric reports and artifact receipts
under its printed scratch directory. `--reuse-host /prior/scratch/host` is allowed
only when every compiled source, header, embedded Info.plist and archive matches.
It starts a new owned process with fresh private stores and repeats all checks.
Unknown future masks/recipes remain cached rendering only; no decoder fallback,
source resizing, tolerance change or repaired fixture substitutes for a failure.
