# Format additions — HEIC, Gray16, typed metadata bridge, float TIFF seam, PDF/SVG (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 11D, drive run 2026-09-18 (all seven gates D1–D7 pass)
- **Originals:** [archive/phase-11d-format-additions-status.md](../archive/phase-11d-format-additions-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-11d-format-additions-status.md](../archive/2026-10/phase-11d-format-additions-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `0e1c496`
- **Carryover:** closes **P7-23**; supplies the float-TIFF codec seam [11F](phase-11f-hdr-float-status.md) qualifies

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
The plan asked for HEIC export and other selected formats, each with a capability/loss matrix; high-depth host-only
I/O (HEIC, 16-bit grayscale → correctly profiled RGB); a typed host-decoder bridge for capture packets and original
PPI (RAW included); float-TIFF codec foundations; and PDF/SVG raster import plus PDF export. "A container depth
claim alone is insufficient": every depth/profile/packet claim must come from an independent decoder.

## Capability / loss matrix
| Flow | Depth the document/file gets | Profile | What is lost, stated |
|---|---|---|---|
| HEIC export | 8 or 10 bits, **measured from the written file** | export's destination, embedded by ImageIO | lossy; IPTC dropped by the host encoder; 16 requested → 8/10 written, reported in `bit_depth` |
| HEIC import | 8 → byte doc, 10/12 → 16-bit typed (12-bit 4:2:0 decodes at full scale; it was never 65520) | file's ICC | since v1.x a gain-map still and a PQ/HLG still open as 32-bit float by default ([v1x-hdr-photos.md](v1x-hdr-photos.md)); with `hdr_photo: "sdr"` a gain-map still opens as its SDR base (`decode_request: "sdr_base"`, disclosed) and PQ/HLG refuses by name; monochrome still refused by the 8F sentence; 12-bit applies to 4:2:0 only — a 12-bit 4:4:4 file (what `heif-enc --lossless` writes) is refused with `HostStillDecoder.noDecodedPixels`, since ImageIO returns it with no pixels (fixture `heic12-444.heic`, gated by D1) |
| Gray16 PNG/TIFF import | 16-bit typed **RGB** | always sRGB | gray-ness; a Gray ICC's curve is applied, not carried |
| Typed packet/PPI bridge | unchanged | unchanged | packets are the reader's reconstruction; exact originals only in the retained file |
| Float TIFF | seam only (11F exposes it) | read, not interpreted | — |
| PDF/SVG import | 8-bit sRGB, one page at a chosen PPI | built-in sRGB | all vector content; one-way raster |
| PDF export | one 8- or 16-bit image page, ZIP or (8-bit) JPEG | ICCBased stream | packets, multipage, vector, PDF/X |
| JPEG 2000 import | 8-bit | file's ICC | a 16-bit container claim still decodes at 8 |
| AVIF import | 8 → byte doc, 10/12 → 16-bit typed | file's ICC | 12-bit opens at full scale: an identity-matrix (nclx matrix 0) file, which ImageIO decodes left-aligned (`v<<4`), is widened `(c << 4) \| (c >> 8)` through the `_left12` layouts; a premultiplied file opens with straight alpha: ImageIO drops its alpha as stored, so the host decodes a copy whose `prem` reference no longer names the primary item and the core divides the colour by alpha |

## Decisions
- **HEIC is host-encoded.** `DR_FORMAT_HEIC = 6`; every flat writer (`dr_image::save`, `save_flat`, `encode_into`)
  refuses it. The export workspace returns a **pending** result whose bytes equal the `"tiff"` request's at the same
  settings (`core/tests/export_host_formats.rs`), `info_json` has `host_encoded: true, intermediate: "tiff"`, and
  `ExportResult::write` / `decode_preview` refuse it. The host transcodes (`HostImageEncoder.heic`) and returns the
  file via `dr_export_result_adopt_host_bytes`, which rebuilds the report from **measured** `carried` bits and
  decoded depth. `ExportCapture.hostFinish` runs at all three encode sites (export capture,
  `LayerExport.encodeLayer`, `ContactSheet.deliver`).
- **HEIC accepts `bit_depth: 16`** and reports what was written; **quality reuses `jpeg_quality`** (one lossy dial).
  `embed_profile` is measured from the file's `colr` box (ImageIO writes `nclx` when ICC-less), never the flag.
- **Gray16 → RGB** (`core/src/gray16_import.rs`), two rules: `SrgbCodes` (no ICC or `sRGB` chunk; codes copied to
  R=G=B unchanged) and `GrayProfile` (Gray ICC: `y = kTRC(g)`, re-encoded through the document's sRGB forward
  curve). Document is always sRGB (a Gray profile has no primaries). Refused: bare `gAMA`/`cHRM`; an RGB ICC on a
  gray image; `cICP`/`mDCV`/`cLLI`. Gray ICC is parsed with `icc_lut::ProofProfile`, **never** `IccProfile::parse`,
  so the RGB path is untouched. TIFF `GrayA` needs exactly one unassociated alpha. A `.rgba8` import choice opens
  through the core's flat open, which converts a Gray ICC through the same per-code rule
  (`gray16_import::encoded_gray`) from the 16-bit samples, rounded to 8 bits.
- **Typed metadata bridge** (`core/src/precision_metadata_edit.rs`): typed `import_metadata` / `set_metadata` /
  `set_resolution` with the byte contracts verbatim (16 MiB packet cap, `Resolution::sane`), sharing one carrier
  parser `doc_metadata_import::carrier_packets`. Typed host stills and typed RAW now carry packets + PPI; loss
  notices shrank to what is still lost (movie frames still disclose the full gap).
- **Float TIFF seam** (`core/src/tiff_float.rs`): decode `SampleFormat=3` at 16/32 bits, 1–4 chunky samples,
  strips/tiles, predictor 1 or 3; encode f32 at 1/3/4 samples, optional Deflate. NaN/∞ counted (`non_finite`) and
  passed through (11F owns the finite policy). Refused by name: F64, planar, >4 samples, mixed widths, multipage,
  non-alpha `ExtraSamples`, and **float-predictor encoding** (the `tiff` crate has no encoder). BigTIFF is read.
- **PDF/SVG import** rasterizes ONE page (`CGPDFDocument`; SVG via `NSImage`/CoreSVG) over transparency into sRGB at
  the chosen PPI (default **300**; 72/144/150/300/600), recording page and PPI in the open's report;
  `inspect_vector`, `open_document` `page`/`ppi`. **Rasterized, not retained.**
- **PDF export** (`app/Sources/PDFExport.swift`) runs the ordinary chain with `format = .tiff` at the page's depth (8
  or 16) and wraps one image at `pixels · 72 / ppi` points with an ICCBased profile and `/SMask` iff alpha.
  Its image stream is byte-equal to the PNG export's pixels at the same depth.
- **PDF JPEG** is the system (ImageIO) JPEG encoder at `jpeg_quality`, 8-bit only, passed through by Quartz as
  `/DCTDecode` with alpha as a separate 8-bit `/SMask` (colour never matted); bits/filter/soft mask are measured from
  the written file (`scripts/verify_export_depth.py`).
- **8F matrix broadened**: `public.avif` and `public.jpeg-2000` join `HostSourceInput.claims`,
  `HostStillDecoder.inspect`, `HostSourceDescriptor`; the actual decoded layout decides, `kCGImagePropertyDepth` is
  only a recorded container claim.

**v1 formats.** Two more export-workspace formats. `avif` is host-encoded exactly as `heic`
is at every site (TIFF intermediate, pending result, adoption of the measured depth and packets;
side cap 16384). `jxl` is lossless JPEG XL encoded by the core (`jxl_encode.rs` over `zune-jpegxl`):
8-bit, sRGB pixels only (a bare codestream carries no profile, so anything else refuses by name),
each side at least 2 px, `format_carries` 0. The core decodes none of the three, so the result's
`host_preview` (true for `heic`, `avif`, `jxl`) tells the host to preview the result's own bytes,
while `host_encoded` keeps meaning "pending an intermediate". Not built: lossy, 16-bit or
ICC-tagged JPEG XL, EXR writing, multipage TIFF, BigTIFF output.

## Rejected alternatives
- Core-side HEIC encoding — no in-crate encoder; the host's is the only honest source, so bytes are adopted.
- A second quality key for HEIC — two things to keep in step.
- Building a "file's own RGB space" for Gray ICC — Gray profiles have no primaries.
- Retaining PDF/SVG as re-rasterizable sources in this slice — needs its own source kind and qualification.
- Gating on ColorSync / `pdftoppm` re-render — recorded, not gated (second CMS / resampler in the loop).

## Consequences
Any new host-encoded format must follow the pending-result + adopt pattern so unadopted intermediates can never
reach disk or preview. Gray inputs stay RGB documents until a gray document mode exists. Tests:
`core/tests/{export_host_formats,gray16_import_tests,tiff_float_tests,precision_metadata_bridge}.rs`; drive
`scripts/verify_phase11d_formats.py`.

## Open items (verified 2026-10-07)
- `TRACKED` — HEVC (`hvc1`/`hev1`) 8-bit/Main10 movie frames (needs a `HostHEVCPreflight` + `ffmpeg` ≤ 4-code
  gate; no such code exists) → plan §11 11D bullet ("HEVC movie frames … stay open").
- `TRACKED` — expanded multiframe ProRes cross-decoder comparison, unqualified at 64 codes → plan §8 8F / §11 11D.
- `TRACKED` — float-predictor encoding (owned predictor-3 writer checked by `tiffinfo`), F16 export → plan §11 11D
  bullet and P7-36 row note.
- `TRACKED` — Gray as export/convert destination, gray document mode, PNG `gAMA` evaluation →
  plan §11 11D bullet ("a gray DESTINATION").
- `TRACKED` — PDF/SVG as retained re-rasterizable sources → plan §11 11D bullet; PDF/X → plan §11 Boundary.
- `TRACKED` — HEIF sequences/animation, AVIF export → plan "Additional formats and sharing" track.
- `DONE` — an untagged CMYK TIFF or JPEG the core opens is converted from the WORKING CMYK profile
  (`core/src/working_cmyk.rs`, set at launch by `WorkingCMYK.install()`: U.S. Web Coated (SWOP) v2 when installed,
  else Generic CMYK) through the embedded-profile path, and `profile_refusal` names it; with none set the naive
  numbers read as sRGB under a sentence saying so. Not covered: a Color Settings UI, an unusable embedded CMYK
  profile (still "read as sRGB"), and gate-refused files (ImageIO's own Generic CMYK). Drive:
  `scripts/verify_issue327_untagged_cmyk.py`.
- `DONE` — a Gray, CMYK or Lab paste into an open document reports its conversion: the reply's `color_conversion`
  and the history row's suffix (CMYK phase 4c, [cmyk-mode-shipped.md](cmyk-mode-shipped.md)).
- `DONE` — a premultiplied AVIF (an `iref` `prem` from the primary) opens with straight alpha. ImageIO drops its
  alpha at 8/10/12 bits, lossless and lossy, so `HostStillDecoder.decode` reads a copy whose `prem` reference's
  `from_item_ID` bytes are 0xFF (`HeifContainer.detachingPremultipliedReference` over the core's
  `premultiplied_reference`, which only locates the field) and declares the decoded samples premultiplied:
  `rgba8_premultiplied`, `rgba16_le_premultiplied`, or the new `rgba16_le_premultiplied_left12` for a 12-bit
  identity-matrix file (expand12 on RGB first, then the division; alpha untouched). Open and re-decode take the same
  path; a file whose reference cannot be detached, or whose alpha is still dropped, keeps `premultipliedRefusal`. No
  admission constant changed. Gate D6 of `scripts/verify_phase11d_formats.py` holds the six classes against
  `avifdec` (alpha equal; RGB 0 where alpha is 0; elsewhere `|Δ| ≤ ceil(k·(2^N−1)/a) + 1`, k = 1 lossless, 2 lossy,
  fixed before the first run).
- `DROPPED` — multipage PDF export.
- `DROPPED` — a 48 MP 10-bit still (HEIC, AVIF) was refused at the 2 GiB test pin by the core's 16-bit
  save-and-reopen admission, not by the host estimate; `scripts/host-still-admission/verify.py`'s A10 opens the wide
  48 MP stills at 8 GiB.
- `DONE` — `scripts/verify_phase11d_formats.py` gate D6 failed when ImageMagick could not read JPEG 2000: the
  `jp2_8` reference was never written (D1–D5 and D7 passed).
- `DONE` — the core's flat open (`core/src/open_profile.rs`) converts a Gray-profiled grayscale file and a
  CMYK-profiled CMYK TIFF to sRGB as it reads them, and the import report's `profile_refusal` names every embedded
  profile it does not keep.
- `DONE` — that refusal reaches every surface: the typed 16-bit open (`open_profile::converted_sentence`,
  `get_color_profile.on_open_refusal`); Place, Replace Contents and Refresh (MCP `disclosures`,
  `contact_sheet_from_sources` `profile_refusals`, and one "Embedded Colour Profile" sheet after the edit commits; a
  no-op import returns and discloses none); and the placed layer's source asset (`get_image_source`), which `.rz`
  persists by conditional word 56 (byte) / 57 (typed) inside 45/46 with an `RSPR` table before `LIDS`. An
  assisted-annotation case file carrying a profile is refused with that refusal.
- `DONE` — one CMYK gate (`open_profile::cmyk_gate`) answers for the open and the import report, for EVERY CMYK
  source whatever its profile: a refused file fails the core open, goes to the platform fallback and gets no
  `profile_refusal`, and a planar or extra-sample CMYK TIFF is not reported "converted". The TIFF gate also refuses
  what `tiff` would refuse under `image`'s limits.
- `DONE` — a CMYK or YCCK JPEG with a CMYK profile is converted through it on the flat open, relative colorimetric
  with BPC like the CMYK TIFF (`core/src/cmyk_jpeg.rs`: Adobe APP14 CMYK inverted, APP14-less CMYK as ink, YCCK
  through the JFIF equations).
- `DONE` — every ImageIO fallback decode of a Gray, CMYK or Lab image reports its conversion to sRGB, worded by the
  core from the file's own bytes (`dr_platform_conversion_sentence`): `converted_sentence` when `decide` would
  convert from the profile, else "The image is {Gray|CMYK|Lab}, with no embedded colour profile Doctored can convert
  from, so it was converted to sRGB as it was read." HEIC/HEIF/AVIF/JXL/JP2 never convert silently:
  `HostStillDecoder` refuses a non-RGB decode.
- `DONE` — File > New from Clipboard reports the same way, worded from the pasteboard's OWN `.tiff` (else `.png`)
  bytes, never `NSImage.tiffRepresentation`'s re-encode, and with the plain sentence when neither type is present; a
  Finder file copy is worded from the file's own bytes (`Bitmap.fileModel(at:matches:)`), though the pasteboard's
  own TIFF/PNG still wins when its model matches the decoded image.
- `DONE` — opaque host stills (RGBX8 `rgba8_opaque`, RGBX16 `rgba16_le_opaque`, packed 10-bit
  `rgb10_packed_le`) open, place and re-decode at their actual depth; gain-map stills open as a disclosed SDR base under `hdr_photo: "sdr"` (float by default since v1.x) and place as one.
- `DONE` — 12-bit AVIF opens at full scale (matrix, AVIF import). No ImageIO property says a file is left-aligned,
  so the host reads the identity matrix and the missing `prem` reference from the container (`heif_container`,
  `dr_heif_primary_color_facts`); alpha is never widened and a sample with low bits set is refused. Gated by D6.
  12-bit 4:4:4 HEIC stays refused.
- `DONE` — host-still admission is measured per depth, not a flat 64 B/px: `HostSourceInput.scratch` =
  `F + a·px + 3·encoded + 64 MiB`, 8-bit F = 187 MiB, a = 18; wide F = 10 MiB, a = 43, the whole open's peak to 100
  MP (1 MiB + 39 to 67 MP before that; `HostStillDecoder.swift`, record beside the constants; rule: 1.25× every
  measured peak). The decoder-buffer floors (12 / 24 B/px) hold at every size. Plain PNG/JPEG/TIFF placement is
  `ImageSourceFile.scratchPerPixel8` = 10, `scratchPerPixel16` = 19. A multi-image still opens its primary image
  (gate A9). Each host-still refusal names its reason, and an over-budget decode names its working memory. Pinned by
  `scripts/host-still-admission/verify.py`.
- `DONE` — a single 8-bit still's WHOLE open is admitted up front: `HostStillDecoder.openScratch` = 30 MiB + 41·px +
  3·encoded + 64 MiB (same rule). A 48 MP 8-bit HEIC open charges 2,180,368,909; verify.py's A1–A3, A7, A8 and A10
  run at 8 GiB.
- `DONE` — the core's flat JPEG/PNG open is admitted whole, `FlatOpenAdmission.openScratch` = 34·px + 3·encoded (48
  MP opens, gate A7); every other 8-bit open admits its first projection after the decode (`firstProjectionBytes`).
- `DONE` — a typed 16-bit document's `get_document` reports `save_preservation` through the byte builder
  (`AgentServer.preservationFields`); a float document does too: its flat 16-bit `save_copy` / Save To tone-maps
  through the export workspace's float stage (a remembered 32-bit export depth falls back to 16), with float-worded
  losses (11F's F11).
- `DONE` — File ▸ Export…, Quick Export, Export Layers… and Contact Sheet… are enabled at 16 bits (Export… and
  Quick Export at 32-bit float too); the PDF's depth and JPEG are Decisions, **PDF export** and **PDF JPEG**.
  Export as PDF… / `export_pdf` writes from a 16-bit or float document; a float page is its tone-mapped projection.
- `DROPPED` — vector-preserving PDF export: it would re-express the whole compositing model in PDF.
- `DONE` — float TIFF in the app (11F, `a3e4f2c`); general PPI editing (P7-26, 12E); typed auxiliary mattes (9G,
  P7-08–P7-11 verified).
- `DROPPED` — HEIC IPTC (host encoder drops it; disclosed); SVG features CoreSVG doesn't draw (disclosed, not
  detected).
