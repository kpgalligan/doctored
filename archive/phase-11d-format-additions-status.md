# Phase 11D — Format additions

Status: **implemented and qualified; all seven drive gates pass.** Three
measurements are RECORDED rather than gated, each with its reason and its
number in the receipt (the ColorSync comparison, the PDF re-render and the
AVIF/JP2-16 rows below); no tolerance was widened after measurement.

This slice owns carryover row **P7-23** and supplies the float-TIFF codec
foundations 11F qualifies.

The plan text this delivers:

> Deliver HEIC export and other selected formats individually, each with a
> capability/loss matrix. Explicitly cover preserving high-depth host-only
> input/output (including HEIC where supported and 16-bit grayscale conversion
> to a correctly profiled RGB document), coordinating 8F frame/source
> decoders. Add the typed host-decoder import bridge for bounded capture
> packets and original PPI, including RAW; independently verify normalization
> and exported metadata. […] Supply shared codec foundations for
> floating-point TIFF; 11F owns its HDR/float interchange qualification so
> ordinary 11D SDR additions can complete independently. A container depth
> claim alone is insufficient. […] Add PDF/SVG raster import and dedicated PDF
> export as their own tested flows.

---

## The capability / loss matrix

Each row is a bounded flow with its own code, its own tests and its own gate
in `scripts/verify_phase11d_formats.py`. "Depth" is what the DOCUMENT gets;
"measured" means an independent decoder produced the number, never Rasterize.

| Flow | Direction | Depth | Profile | Alpha | Packets / PPI | What is lost, stated |
|---|---|---|---|---|---|---|
| **HEIC** | export | 8 or 10 bits, **measured from the written file** | the export's destination profile, embedded by ImageIO; the report says "carried" when the WRITTEN file carries one (a host encoder may re-serialize the same space, so byte equality is the drive's question, asked of the decoded profiles — measured equal) | preserved where the encoder writes it | EXIF, XMP and the DPI travel; **IPTC is dropped by the host encoder** and the result says so | lossy compression at the request's quality; 16 bits requested is 8 or 10 written, and `bit_depth` reports which |
| **HEIC** | import | 8-bit → byte document, 10-bit → 16-bit typed document | the file's own ICC (P3 included) | preserved | reconstructed through the typed bridge below | 12-bit and non-RGB (monochrome, PQ/HLG) are refused by the existing 8F sentences, never substituted |
| **Gray16 PNG/TIFF** | import | 16-bit typed RGB | **always sRGB** (see the interpretation below) | `La16` / `GrayA16` preserved exactly | the container's own, through the shared scanner | the file's gray-ness: the document is RGB, and a Gray ICC's curve is applied rather than carried |
| **Typed packet/PPI bridge** | import | unchanged | unchanged | unchanged | EXIF/XMP/IPTC and the original PPI reconstructed INTO the typed document | the packets are the platform reader's RECONSTRUCTED view; the exact original bytes live only inside the retained original file |
| **Float TIFF** | seam only | — | ICC read, not interpreted | `ExtraSamples` 1 or 2 read | packets and density read through the shared scanner | not reachable from the app at all: no FFI, no document mode (11F) |
| **PDF / SVG** | import | 8-bit sRGB | built-in sRGB | preserved (drawn over transparency) | none; the page and ppi are recorded in the open's report | everything vector: text, paths, colour management, and any re-rasterization |
| **PDF** | export | one 8-bit image page | embedded as an ICCBased stream | written as an `/SMask` | none | capture packets; multipage, vector content, PDF/X and 16-bit images |
| **JPEG 2000** | import | 8-bit → byte document | the file's own ICC | preserved | as any other host still | a container that CLAIMS 16 bits still gets what ImageIO decoded (8, measured); two JPEG 2000 decoders differ by up to 1 code |
| **AVIF** | import | — | — | — | — | **DISCLOSED, not supported**: ImageIO hands back an RGBX 8-bit or a packed 10-bit layout the qualified packing seam does not admit, so the open refuses by sentence and substitutes nothing |

---

## The contracts, one per flow

### 1. HEIC is HOST-ENCODED, and the core says so

`RZ_FORMAT_HEIC = 6` exists, and **every flat writer refuses it** with one
sentence: `rz_image::save`, `save_flat` and `encode_into` all return *"HEIC is
encoded by the host through the export workspace (format \"heic\")"*. The
export workspace is the only path that accepts it, and what it returns is a
**pending** result:

* its bytes are the `"tiff"` request's bytes at the same settings — an
  EQUALITY, tested in `core/tests/export_host_formats.rs` over four settings
  groups and a document carrying EXIF + XMP + IPTC;
* `info_json` carries `"host_encoded": true, "intermediate": "tiff"`;
* `ExportResult::write` and `decode_preview` both refuse it by name, so a
  pending intermediate **cannot reach disk or a preview**;
* the host transcodes those bytes (`HostImageEncoder.heic`) and hands the
  written file back through `rz_export_result_adopt_host_bytes`, which
  rebuilds the packet report from the MEASURED `carried` bits and the depth
  the written file decodes at.

`ExportCapture.hostFinish` runs at the three encode sites — the export
capture, `LayerExport.encodeLayer` and `ContactSheet.deliver` — so every
consumer of a result's bytes, preview or info sees an adopted result.

**Recorded decision: HEIC accepts `bit_depth: 16`.** The request asks for
high-depth HEIC output "where supported", and the honest way to answer is to
let the request through and report what was written: the sheet's depth popup,
`AgentServer+Preservation`'s capability report and the core's `validate()` all
admit it, `PrecisionExportCapture.losses` names the depth sentence, and the
result's `bit_depth` is the decoded `bitsPerComponent` of the file. An
8-bit-only host outcome is therefore REPORTED, not assumed away.

**Recorded decision: HEIC's quality dial is the existing `jpeg_quality` key.**
The request has exactly one lossy-quality field and adding a second spelling
for the same number would be a second thing to keep in step.

### 2. Gray16 → a correctly profiled RGB document

`core/src/gray16_import.rs` holds the whole interpretation, and it is exactly
two rules:

* **`SrgbCodes`** — no ICC, or an `sRGB` chunk: the samples are read as
  sRGB-encoded gray and copied into R = G = B **unchanged**. No pixel
  conversion happens, so the codes are preserved exactly.
* **`GrayProfile(curve)`** — a Gray ICC: `y = curve(g / 65535)` is the linear
  luminance the file's own `kTRC` says the code carries, `c` is `y` encoded by
  the DOCUMENT profile's forward curve, and the stored code is
  `round(65535 · c)`, in all three channels.

The document profile is **always sRGB**: a Gray profile has a tone curve and a
white point and no primaries, so there is no "the file's own RGB space" to
build, and sRGB round-trips a neutral exactly.

The chunk-by-chunk contract, checked in `core/tests/gray16_import_tests.rs`
over fixtures the test assembles byte by byte:

| the file says | the result |
|---|---|
| nothing | `SrgbCodes`, with a warning that says so |
| `sRGB` chunk | `SrgbCodes` (the chunk is a declaration, not a profile) |
| `gAMA` or `cHRM` alone | **refused**: *"PNG gamma/chromaticities require an explicit Gray ICC or sRGB declaration"* |
| `gAMA` + `sRGB` | `SrgbCodes` (`gAMA` is never evaluated, as on the RGB path) |
| a Gray ICC | `GrayProfile`, with a warning naming the profile |
| a Gray ICC + `gAMA` | `GrayProfile` |
| an RGB ICC on a gray image | **refused**: *"a grayscale image must carry a Gray ICC profile or none"* |
| `cICP` / `mDCV` / `cLLI` | refused, as today |

Gray ICC bytes are parsed with `icc_lut::ProofProfile` and **never**
`IccProfile::parse` — the gray branch is taken before that shared step, so the
RGB path's bytes and sentences are untouched. TIFF adds one rule: a `GrayA`
image must declare exactly one unassociated alpha sample, and a plain `Gray`
one none.

**Recorded decision: a `.rgba8` import choice keeps today's byte behaviour**
(the `image` crate promotes `L16` to RGBA8 and ignores a Gray ICC). Changing
it would move bytes an existing document already has.

### 3. The typed host-decoder bridge

`core/src/precision_metadata_edit.rs` adds the three typed twins of the byte
metadata edits — `import_metadata`, `set_metadata`, `set_resolution` — with
the byte contracts verbatim (the 16 MiB packet cap, `None` for no change,
`Resolution::sane`), sharing ONE carrier parser
(`doc_metadata_import::carrier_packets`, extracted for this) so the two depths
cannot read a carrier two ways.

With them, the typed still branch of `HostSourceImport` and the typed branch
of `RawSourceWork` attach the platform reader's packets and the file's own PPI
exactly as their byte twins do, through the same two facade calls. The loss
notice shrank accordingly:

* a typed host STILL no longer claims packets and PPI are missing; it states
  that they are the reader's reconstructed view;
* a typed RAW states that the exact camera metadata — MakerNotes and private
  fields included — lives only inside the retained original file;
* a selected MOVIE FRAME still discloses the full gap, because there is no
  still-image carrier to read.

### 4. The float TIFF seam, and its boundary

`core/src/tiff_float.rs` decodes `SampleFormat = 3` at 16 or 32 bits, 1–4
samples, chunky, strips or tiles, predictor 1 or 3, any compression the `tiff`
crate reads; and encodes f32 at 1, 3 or 4 samples with optional Deflate.
NaN and infinity are **counted** (`non_finite`) and passed through unchanged.

**It is not reachable from the app**: no FFI export, no document mode, no
import choice, no export format. Deciding what a value above 1.0 MEANS is
11F/P7-36's question. What 11D answers is "can this crate read and write the
container exactly", and `core/tests/tiff_float_tests.rs` answers it against
files the test assembles byte by byte, including the predictor's shuffle.

Refused with the element named: SampleFormat ≠ 3, bits ∉ {16, 32} (F64), a
planar file, more than four samples, mixed widths, BigTIFF, multipage, an
`ExtraSamples` that is not one associated or unassociated alpha. **Float
predictor ENCODING** is refused because the `tiff` crate has no such encoder;
its acceptance criterion is an owned predictor-3 writer checked against
`tiffinfo`.

### 5. PDF/SVG import and PDF export

Import rasterizes ONE page (PDF: `CGPDFDocument`; SVG: `NSImage`, which
reaches CoreSVG) at a chosen PPI over **transparency**, into an sRGB document
whose resolution is the PPI it was drawn at. The page and the PPI are recorded
in the open's metadata report, so the same import is repeatable.
`inspect_vector` reports the page count and each page's size in points, and
`open_document` takes `page` and `ppi` (refused by name on any other path).

**Recorded decision: rasterized, not retained.** Keeping a PDF/SVG as a
re-rasterizable retained source is a real feature with its own source kind,
native record and qualification; this slice ships the honest one-way import
instead and says so in the report.

**Recorded decision: 300 ppi is the default**, with 72/144/150/300/600 offered
— the ordinary print figure for a placed page.

Export writes ONE page holding ONE 8-bit image: the same pixels the 11A chain
produces at the same settings, drawn at the document's own print size
(`pixels · 72 / ppi` points), with the profile embedded as an ICCBased stream.
It runs the ordinary export chain with `format = .tiff, bit_depth = 8` rather
than re-implementing any delivery setting.

### 6. The 8F matrix, broadened

`public.avif` and `public.jpeg-2000` join the host still route
(`HostSourceInput.claims`, `HostStillDecoder.inspect`,
`HostSourceDescriptor`), and the document types list gains AVIF, JPEG 2000,
PDF and SVG. The 8F rule is unchanged and is what admits them: the ACTUAL
decoded layout, ICC and alpha decide, `kCGImagePropertyDepth` is a container
claim recorded beside the evidence, and an HDR/extended-range/non-RGB input
refuses with the existing sentence.

The expanded multiframe **ProRes cross-decoder comparison remains
unqualified** at the unchanged 64-code bound; nothing in this slice re-ran it
or moved it.

---

## Verification

* Rust: `core/tests/export_host_formats.rs`, `gray16_import_tests.rs`,
  `tiff_float_tests.rs`, `precision_metadata_bridge.rs`, plus the edited
  expectations in `doc_tests.rs`, `metadata_tests.rs` and
  `export_pipeline.rs`. Every fixture is assembled in the test — the PNG
  container with its own stored-deflate writer and CRC, the classic TIFF at
  both endiannesses, the Gray ICC profiles, the float predictor's shuffle —
  and every expectation is arithmetic written beside it.
* The drive: `scripts/verify_phase11d_formats.py`, gates D1–D7, receipts
  under `~/devel/rasterize-fixtures/phase11-receipts/11d/`. Its tolerances are
  fixed in the file's own header before anything is measured, and a tolerance
  whose value depends on its subject names that subject: **T3 is measured on
  a photographic plate; the per-pixel-noise plate is exported in the same
  gate and RECORDED, not gated** (`T3_HEIC_PLATE`), so the bound and what it
  is a bound about are fixed together.

### What the run measured (2026-09-18)

| gate | the claim | measured |
|---|---|---|
| D1 | HEIC export vs the PNG export of identical settings, on the PHOTOGRAPHIC plate (T3: mean ≤ 2, p99 ≤ 8 on that plate; the per-pixel-noise plate is recorded, not gated) | mean **1.14**, p99 **4**, max 7; ICC bytes EQUAL; Orientation 1; XResolution 300/1 |
| D1 | the depth `heif-info` reports == the result's `bit_depth` | both **10** for a typed `bit_depth: 16` request, both 8 for the byte one |
| D1 | 8-bit HEIC import vs `magick` | **exact** (max 0) |
| D1 | 10-bit HEIC import (T1: ≤ 2 codes at 10 bits, mean ≤ 0.5) | max **1**, mean **0.48** |
| D1 | 12-bit HEIC | **refused** ("the full image provider disagrees with its declared buffer size") — recorded, never substituted |
| D1 | a PQ HEIC | **refused** by the existing HDR sentence |
| D1 | `contact_sheet` and `export_layers` HEIC deliveries | both written and parsed by `heif-info` — no TIFF in a `.heic` |
| D2 | Gray Gamma 2.2 vs lcms (T2: max ≤ 514 u16, mean ≤ 128) | max **10**, mean **4.5** |
| D2 | untagged, `sRGB`-chunk and GrayA16 | **exact**, alpha exact |
| D3 | the exported TIFF16's authored EXIF (T4) | Make, Model, DateTimeOriginal, both resolutions and Orientation 1 all **equal** |
| D4 | a Rust-written float TIFF read by `tiffinfo` and `magick` | both read it; the seam's own round trip is exact |
| D5 | PDF import vs `pdftoppm -r 144` outside a 1-px band (T5: ≤ 2 codes) | **exact** (max 0); the analytic rectangle gate is exact too |
| D5 | the PDF export's image stream vs the PNG export | **byte-for-byte equal**; one page, MediaBox 15.36 × 11.52 pt, `/BitsPerComponent 8`, `/Filter /FlateDecode`, an ICCBased stream, `/SMask` present iff the document has alpha |
| D6 | 8-bit JPEG 2000 vs `magick` | **exact** |
| D7 | the master across every export | `get_document` identical, `.rz` `save_copy` sha256 identical, `tools/list` parses with both new tools (208) |

### Recorded, not gated — and why

* **ColorSync (`sips --matchTo`) on the Gray ICC case.** Measured at the
  8-bit scale it writes: max **6** codes, mean **0.38**. lcms and the
  analytic curve agree with this build to 10 u16 (0.04 of an 8-bit code), so
  the disagreement is ColorSync's own gray policy — it exposes no intent or
  black-point switch — and it is reported as a second CMS rather than as a
  gate. The same treatment 11C's T4 gives `sips`.
* **The PDF export's `pdftoppm` re-render.** Fixed bound 1 code, measured max
  **4**, mean 1.58. A re-render puts poppler's own image resampling between
  the two sides; the exact claim is the image stream's byte equality with the
  PNG export, which passes. The bound and the measurement are both in the
  receipt.
* **AVIF.** ImageIO decodes an 8-bit AVIF into an RGBX (`noneSkipLast`)
  layout and a 10-bit one into a packed 30-bit layout; the qualified 8F
  integer-packing seam admits neither, so the open **refuses by sentence and
  substitutes nothing**. The container is claimed nowhere: it is in the
  document-types list and in the 8F route, and it says why it cannot be
  opened.
* **A 16-bit-claiming JPEG 2000.** `opj_compress` writes one; ImageIO decodes
  it at 8 bits, so the row compares two independent JPEG 2000 DECODERS (max
  **1** code) rather than this build's preservation — the plan's own "a
  container depth claim alone is insufficient", observed.
* **`embed_profile: false` on a HEIC.** ImageIO always declares the colour:
  an ICC-less intermediate is written with an `nclx` declaration instead of
  an embedded profile (`heif-info` says `color profile: nclx`, `magick`
  extracts none). The profile bit is therefore MEASURED from the file's own
  `colr` box rather than from the decoder's synthesized colour space — which
  is identical for both files, and whose ICC blob appears nowhere in the
  bytes — and never from the request's flag. The drive gates `carried`
  against `magick`'s extraction for an `embed_profile: false` AND a profiled
  delivery, so the equality is checked in both directions.
* **The high-frequency plate's HEIC delta** (max 177, mean 29) is kept beside
  T3's photographic number: it is what 4:2:0 chroma subsampling does to
  per-pixel noise, and it is why T3 is measured on a picture.
* **A typed document's `get_document` carries no `save_preservation` block**
  at all (a pre-existing shape this slice did not change), so the typed HEIC
  depth sentence in `PrecisionExportCapture.losses` has no MCP surface. What
  is observable — and what the claim rests on — is that the result's
  `bit_depth` is the depth an independent reader finds.

### The one in-test tolerance, and what it is

`gray16_import_tests` compares the crate's re-encode against the **analytic**
sRGB transfer function written in the test, with a 4-code (u16) bound. That is
not the conversion's accuracy: this build's sRGB profile carries a SAMPLED
`curv` TRC, and the bound is the difference between that table and the
closed-form IEC curve. The measured maximum over the test's codes is 3 —
0.012 of an 8-bit code — and the endpoints are exact and checked separately.

---

## What stays open

* **HEVC (`hvc1`/`hev1`) 8-bit and Main10 movie frames.** Phase 2 of the
  matrix: a `HostHEVCPreflight` (hvcC plus in-band SPS bit-depth/chroma
  agreement, the AVC walk) and an `ffmpeg -pix_fmt rgb24/rgb48le` flat-patch
  comparison at a fixed ≤ 4-code (8-bit scale) gate.
* **The expanded multiframe ProRes cross-decoder comparison**, unqualified at
  the unchanged 64-code bound.
* **Float TIFF in the app** — document mode, FFI, HDR readouts, tone-mapped
  delivery — is 11F/P7-36 over this seam. Float-predictor ENCODING and F64 are
  refused with a sentence; the acceptance criterion for the former is an owned
  predictor-3 writer tested against `tiffinfo`.
* **Gray as an export or convert DESTINATION**, a grayscale document mode,
  byte-path Gray ICC honouring and PNG `gAMA` evaluation. Acceptance: a
  gray-encoded output path.
* **PDF**: multipage, vector-preserving export, PDF/X, text, JPEG-compressed
  or 16-bit images, and PDF/SVG as retained re-rasterizable sources.
  **SVG**: whatever CoreSVG does not draw (web fonts, external references),
  disclosed rather than detected.
* **AVIF import.** Acceptance: admit ImageIO's 8-bit RGBX (`noneSkipLast`)
  and packed 10-bit layouts in the host integer-packing seam
  (`HostStillDecoder.layout` plus the core's `host_pixels` layouts), with the
  same exact-sample comparison the JPEG 2000 row passes. Until then the open
  refuses and substitutes nothing.
* **Surfacing the typed loss table over MCP**, so a typed document's HEIC
  depth sentence is readable there as the byte document's capability report
  is.
* **HEIC IPTC, HEIF sequences and animation, 12-bit HEIC import/export, AVIF
  export.** General PPI editing stays 12E; typed auxiliary mattes stay 9G.
