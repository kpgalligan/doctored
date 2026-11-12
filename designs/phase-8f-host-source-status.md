# Phase 8F — retained host image and frame sources (ADR)

- **Status:** Accepted — open items remain ([#11](https://github.com/kpgalligan/doctored/issues/11), [#15](https://github.com/kpgalligan/doctored/issues/15))
- **Phase / dates:** 8F, 2026-09-11
- **Original:** [archive/phase-8f-host-source-status.md](../archive/phase-8f-host-source-status.md) — full history, measurements, receipts
- **Key commits:** `9b3f65c` (follows 8E `23cf25d`)
- **Removed 2026-10-10:** retained host sources. A host still or movie frame is still decoded under the decoder
  contract below (`HostStillDecoder.swift`, `HostVideoDecoder.swift`), but it opens or places as an ordinary
  raster layer: no encoded original or decoded master is kept, and the frame sheet, `get_host_source`,
  `set_host_frame`, the `live_photo` layer kind, the probe's Swift and drives and `scripts/verify_phase8f.py`
  are gone. An old `.rz` opens such a layer as its pixels. See
  [the removal ADR](convert-to-smart-object-status.md).

## Context
Host-only decoders (JPEG XL, HEIC, ProRes/H.264 movies, Live Photo pairs) were previously decoded once to byte
pixels, and legacy Live Photo layers stored file paths. 8F extends the [retained-source model](phase-8-retained-source-status.md)
to these inputs: keep the exact encoded originals and the decoder's *actual* samples, allow re-selecting a frame
later without the original files or a decoder at reopen, and disclose genuinely byte-valued input. It verified P7-03.

## Decisions
- **Retain exact encoded originals and the actual decoded master** (samples, ICC, alpha) independently of the
  document fallback depth; a genuine byte source stays a byte master in a 16-bit document. Actual full decoder
  output determines storage/layout/profile; container depth claims are recorded, never trusted.
- **Core packs host buffers** (`host_pixels.rs`, `ffi_host_pixels.rs`) from the declared integer channel order,
  endianness, alpha mode, row padding and exact orientation. Unsupported profile/layout, or an unverified
  high-depth→byte decode, refuses; the HDR still's f32 layouts (`rgba32f_le`) go only to the float intake
  (`float_host_intake.rs`), and the byte and typed packers refuse them.
- **Strict decoder contract + rational frame identity.** Descriptors save decoder/codec identity and exact
  rational requested/resolved time. Movie requests pick the greatest PTS ≤ request inside the track's
  start-inclusive/end-exclusive interval; `value/scale` input stays rational, decimal seconds round explicitly to
  microseconds. A selected still never falls through to a movie frame. Changed/unknown decoders keep saved
  rendering and refuse new decoding; before re-edit the host checks original hashes and saved master
  dimensions/depth/ICC.
- **Frame sheet** (`HostFrameSheetController.swift`) separates Original Still from Movie Frame; Preview Frame
  prepares the full candidate and Apply publishes those same pixels (no second decode); same resolved frame adds
  no Undo. A separate fit keeps the source canvas stable when still/movie dimensions differ, composed with the
  user affine into one binding. Same-basename still+movie pairing is disclosed as filename pairing, not an Apple
  asset-ID match.
- **MCP:** `get_host_source`, `set_host_frame` (shares the UI publication path; legacy Live Photo tool names
  forward to it). Actions retain exact frame identity, original hashes and decoder version. Legacy path-based
  layers show cached pixels; an explicit edit captures their available files (old description kept in Undo).
- **Persistence:** byte native **v21** / typed **v22** add a common deduplicated binary resource pool (`RBIN`,
  `native_source_resources.rs`) shared with retained RAW originals; typed 22 keeps the artwork table. One
  original or an ordered still/movie pair per source; generic framing allows 4 resources (used by 8G masks).
  Caps: 256 MiB per resource, 512 MiB unique combined RAW/attachment originals, 64 KiB envelope with 48 KiB
  opaque state. Movies: one stable track/format, ≤120 s, ≤4,096 samples; AVC inspection caps packets/NALs/SEI.
  Reopen never invokes a host decoder.
- **Publication** goes through the single worker with captured document/target/selection checks and one
  central Undo; presence guards never depend on an admitted metadata read; attachment writers answer to the
  layer's pixel lock (12E-2b). Individual ImageIO/codec calls are noncancellable; loops and core stages cooperate.

## Rejected alternatives
- Implicit ImageIO animation composition from image-count APIs — not implied; general animation is out of scope.
- Widening the 64-code ProRes bound after the cross-decoder comparison failed — the bound is kept and the result recorded as unqualified.

## Consequences
- Initial corpus is SDR only: lossless Display P3 JXL, HEIC 8/10-bit, ProRes 4444 with alpha, H.264 byte frames,
  paired JPEG/HEIC stills (11D later added AVIF and JPEG 2000). New codecs must be qualified on decoded evidence.
- Qualification claims exact preservation of *actual* AVFoundation output, not cross-decoder bit equality.

## Qualification
Authored Display P3 JXL RGBA16 preserves all 147,456 components and ICC exactly; the single ProRes calibration
case stays within 64 u16 codes (52/27/60); 56 saved masters / 10.9 M source components and 9.4 M external-MCP
components exact through native/snapshot/crash recovery after deleting originals. The expanded multiframe ProRes
cross-decoder comparison **failed** its 64-code bound (79 codes) and is recorded as unqualified.
Reproduction, at `3e05aea6` and earlier: the host-source probe's Swift and drives and `scripts/verify_phase8f.py`,
removed with the feature; `scripts/host-source-probe/` keeps only its fixture writer, native reader and the
`HostApp` launcher support.

## Open items (verified 2026-09-26)
- `DONE` — corpus re-authored and matched the 8F record (2026-09-27); the probe's prepare step and drive were removed 2026-10-10.
- `DONE` — why the probe passed while the app still refused opaque stills: its "opaque" stills are not opaque-layout files. `fixtures.py` always writes RGBA PNGs (`opaque=True` only sets alpha to max), and `magick` keeps that alpha plane, so `rgb8.heic`, `rgb10.heic` and `pair/Live.heic` carry alpha (`heif-info`: "alpha channel: yes"). ImageIO decodes them to `rgba8` / `rgba16_le`, and `pair-jpeg/Live.jpg` to 24-bit `rgb8`, all admitted even before opaque stills opened (measured on the re-authored corpus with `HostStillDecoder`'s options, 2026-09-27). The `Live.heic` recipe is unchanged since `9b3f65c`. The lost `/tmp` `rgb8`/`rgb10` recipe is unknown, but the re-authored HEIC10 matches its decoded record. The probe never accepts a refusal: `opened()` goes through `Client.call`, which asserts `isError` is false. It still does not cover RGBX/packed-10 layouts; the committed `core/tests/fixtures/host_stills_opaque/` fixtures gated by `verify_phase11d_formats.py` do.
- `DROPPED` (2026-10-30) — Expanded multiframe ProRes cross-decoder conformance: retained host sources were removed 2026-10-10 and frames land as plain pixels, so decoder agreement has no user-visible effect.
- `DONE` — SDR HEVC movie frames: `hvc1` at 8 or 10 bits 4:2:0 (`HostHEVCPrecision.swift` reads `hvcC`, `HostHEVCPreflight.swift` checks every NAL header) open and place at their depth; `hev1` refuses by name, as macOS's decoder does not read it. HLG and PQ HEVC Main10 frames open as 32-bit float documents by the PQ/HLG photo rule (`HDRMovieFrameOpen.swift`); placing one, or a pair holding one, refuses. Other codecs with an HDR transfer refuse. `scripts/verify_movie_frames.py`.
- `OPEN` [#15](https://github.com/kpgalligan/doctored/issues/15) — Dolby Vision movie frames refuse by name until a real clip shows what AVFoundation hands back for a profile 8.4 track.
- `DONE` — HDR host stills open as 32-bit float documents (`core/src/float_host_intake.rs`).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — broader size and cancellation (carryover P7-32 / P7-33).
- `DONE` — Typed capture packets/PPI for host stills (11D typed metadata bridge; selected movie frames still disclose the gap by design).
- `DONE` — Typed auxiliary matte import (9G, P7-08).
- `DONE` — Retained-source group/perspective/whole-document geometry (12F-2, `07d9153`; Canvas Size by 12F-1).
- `DONE` — Arbitrary placement slot and pixel-lock gating of attachment writers (12E-2b).
