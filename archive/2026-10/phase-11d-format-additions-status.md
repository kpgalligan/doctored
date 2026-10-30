# Format additions — HEIC, Gray16, typed metadata bridge, float TIFF seam, PDF/SVG (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 11D, drive run 2026-09-18 (all seven gates D1–D7 pass)
- **Original:** [archive/phase-11d-format-additions-status.md](../archive/phase-11d-format-additions-status.md) — full history, measurements, receipts
- **Key commits:** `0e1c496`
- **Carryover:** closes **P7-23**; supplies the float-TIFF codec seam [11F](phase-11f-hdr-float-status.md) qualifies

## Context
The plan asked for HEIC export and other selected formats, each with a capability/loss matrix; high-depth host-only
I/O (HEIC, 16-bit grayscale → correctly profiled RGB); a typed host-decoder bridge for capture packets and original
PPI (RAW included); float-TIFF codec foundations; and PDF/SVG raster import plus PDF export. "A container depth
claim alone is insufficient": every depth/profile/packet claim must come from an independent decoder.

## Capability / loss matrix
| Flow | Depth the document/file gets | Profile | What is lost, stated |
|---|---|---|---|
| HEIC export | 8 or 10 bits, **measured from the written file** | export's destination, embedded by ImageIO | lossy; IPTC dropped by the host encoder; 16 requested → 8/10 written, reported in `bit_depth` |
| HEIC import | 8 → byte doc, 10/12 → 16-bit typed (12-bit 4:2:0 decodes at full scale; it was never 65520) | file's ICC | a gain-map still opens as its SDR base (`decode_request: "sdr_base"`, disclosed, original retained); PQ/HLG/monochrome still refused by the 8F sentences; 12-bit applies to 4:2:0 only — a 12-bit 4:4:4 file (what `heif-enc --lossless` writes) is refused with `HostStillDecoder.noDecodedPixels`, since ImageIO returns it with no pixels (fixture `heic12-444.heic`, gated by D1) |
| Gray16 PNG/TIFF import | 16-bit typed **RGB** | always sRGB | gray-ness; a Gray ICC's curve is applied, not carried |
| Typed packet/PPI bridge | unchanged | unchanged | packets are the reader's reconstruction; exact originals only in the retained file |
| Float TIFF | seam only (11F exposes it) | read, not interpreted | — |
| PDF/SVG import | 8-bit sRGB, one page at a chosen PPI | built-in sRGB | all vector content; one-way raster |
| PDF export | one 8- or 16-bit image page, ZIP or (8-bit) JPEG | ICCBased stream | packets, multipage, vector, PDF/X |
| JPEG 2000 import | 8-bit | file's ICC | a 16-bit container claim still decodes at 8 |
| AVIF import | 8 → byte doc, 10/12 → 16-bit typed | file's ICC | 12-bit opens at full scale: an identity-matrix (nclx matrix 0) file without premultiplied alpha, which ImageIO decodes left-aligned (`v<<4`), is widened `(c << 4) \| (c >> 8)` through the `_left12` layouts; a premultiplied file loses its alpha in ImageIO |

## Decisions
- **HEIC is host-encoded.** `RZ_FORMAT_HEIC = 6`; every flat writer (`rz_image::save`, `save_flat`, `encode_into`)
  refuses it. The export workspace returns a **pending** result whose bytes equal the `"tiff"` request's at the same
  settings (`core/tests/export_host_formats.rs`), `info_json` has `host_encoded: true, intermediate: "tiff"`, and
  `ExportResult::write` / `decode_preview` refuse it. The host transcodes (`HostImageEncoder.heic`) and returns the
  file via `rz_export_result_adopt_host_bytes`, which rebuilds the report from **measured** `carried` bits and
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
  passed through (11F owns the finite policy). Refused by name: F64, planar, >4 samples, mixed widths,
  multipage, non-alpha `ExtraSamples`, and **float-predictor encoding** (the `tiff` crate has no encoder).
  BigTIFF reads.
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

## Open items (verified 2026-09-26)
- `TRACKED` — HEVC (`hvc1`/`hev1`) 8-bit/Main10 movie frames (needs a `HostHEVCPreflight` + `ffmpeg` ≤ 4-code
  gate; no such code exists) → plan §11 11D bullet ("HEVC movie frames … stay open").
- `TRACKED` — expanded multiframe ProRes cross-decoder comparison, unqualified at 64 codes → plan §8 8F / §11 11D.
- `TRACKED` — float-predictor encoding (owned predictor-3 writer checked by `tiffinfo`), F16 export → plan §11 11D
  bullet and P7-36 row note.
- `TRACKED` — Gray as export/convert destination, gray document mode, PNG `gAMA` evaluation →
  plan §11 11D bullet ("a gray DESTINATION").
- `TRACKED` — PDF/SVG as retained re-rasterizable sources → plan §11 11D bullet; PDF/X → plan §11 Boundary.
- `TRACKED` — HEIF sequences/animation, AVIF export → plan "Additional formats and sharing" track.
- `DONE` — the byte path's Gray ICC: the core's flat
  open (`core/src/open_profile.rs`) converts a Gray-profiled grayscale file and a CMYK-profiled CMYK TIFF to sRGB
  as it reads them, and the import report's `profile_refusal` names every embedded profile it does not keep.
- `DONE` — the typed 16-bit open's Gray conversion
  words its warning through the flat open's sentence (`open_profile::converted_sentence`), and the host shows it as
  the open's profile refusal (`get_color_profile.on_open_refusal`) when the typed `import_warnings` carry it.
- `DONE` — Place, Replace Contents and Refresh name the
  core open's profile refusal in the MCP reply's `disclosures`, `contact_sheet_from_sources` in `profile_refusals`,
  and an assisted-annotation case file carrying a profile is refused with it.
- `DONE` — the placed layer keeps that refusal: Place and
  Replace relay it onto the source asset, `get_image_source` and Layer > Image Source… report it, and `.rz` persists
  it by conditional word 56 (byte) / 57 (typed) inside 45/46 with an `RSPR` table before `LIDS`.
- `DONE` — the UI shows it too: Place Embedded/Linked,
  Replace Contents and Refresh present the sentence once, after the edit commits, as an informational "Embedded
  Colour Profile" sheet (the MCP `disclosures` text verbatim); a no-op import returns and discloses none.
- `OPEN` — item 2's header gate done (the import report
  shares the open's one CMYK gate, `open_profile::cmyk_gate` through `cmyk_convertible`, so a planar or extra-sample
  CMYK TIFF is not reported "converted"); item 1 (the default for an untagged CMYK TIFF, and its CMYK JPEG twin) open.
- `DONE` — a CMYK or YCCK JPEG with a CMYK profile is
  converted through it on the core's flat open, relative colorimetric with BPC like the CMYK TIFF:
  `core/src/cmyk_jpeg.rs` reads the raw planes through `zune-jpeg` with `image`'s own options (Adobe APP14 CMYK
  inverted, APP14-less CMYK as ink, YCCK through the JFIF equations), and the import report says "converted"
  (`core/tests/open_profile_tests.rs`; the probe's ImageIO-written CMYK JPEG matches ColorSync).
- `DONE` — the open asks the one CMYK gate
  (`open_profile::cmyk_gate`) for EVERY CMYK source, whatever its profile, so a CMYK file the gate refuses fails the
  core open (and goes to the platform fallback) and the report gives it no `profile_refusal` — no more "ignored" for
  a planar file the core never opened. The TIFF gate also refuses what `tiff` would refuse under `image`'s limits
  (the 512 MiB output buffer, a strip or tile past the allowance beside it).
- `DONE` — every ImageIO fallback decode of a Gray, CMYK
  or Lab image reports its conversion to sRGB (`RasterImage.decoded(from:)` → `Bitmap.platformConversion(at:)`),
  worded by the core from the file's own bytes (`rz_platform_conversion_sentence` over `rz_image::embedded_icc`,
  `open_profile::platform_conversion_sentence`): `converted_sentence` naming the profile when `decide` would convert
  such samples from it (S1/S2), else "The image is {Gray|CMYK|Lab}, with no embedded colour profile Rasterize can
  convert from, so it was converted to sRGB as it was read." — so an untagged, B2A-only or unreadable-table
  gate-refused CMYK file is reported too. The import report's `platform_profile_conversion` key is gone.
- `DONE` — Gray and Lab platform decodes report the same
  way (a Lab TIFF the core cannot read gets the plain Lab sentence). HEIC/HEIF/AVIF/JXL/JP2 never convert silently:
  `HostStillDecoder` refuses a non-RGB decode.
- `DONE` (New from Clipboard) — File > New from Clipboard
  reports a Gray, CMYK or Lab paste the same way, worded from the pasteboard's OWN `.tiff` (else `.png`) bytes —
  never `NSImage.tiffRepresentation`'s re-encode, which writes a generic profile the source never had — and with the
  plain sentence when neither type is present. Paste into an OPEN document (CoreGraphics converts into the document's
  space, as for every external paste) reports nothing yet; how it should is a maintainer decision.
- `DONE` — a Finder file copy (a file URL only) is worded
  from the file's own bytes (`Bitmap.fileModel(at:matches:)`, then `platformConversion(at:)`), so the embedded profile
  is named; the pasteboard's own TIFF/PNG still wins when its model matches the decoded image.
- `DONE` — opaque host stills (RGBX8 `rgba8_opaque`, RGBX16 `rgba16_le_opaque`, packed 10-bit
  `rgb10_packed_le`) open, place and re-decode at their actual depth; gain-map stills open as a disclosed SDR base.
- `DONE` — 12-bit AVIF opens at full scale. ImageIO
  decodes a 12-bit AVIF left-aligned (`v << 4`, white 65520) exactly when its primary item declares the identity
  matrix (`colr`/`nclx` matrix 0) and it carries no `prem` reference, and no ImageIO property says so; the host reads
  that from the container (`heif_container`, `rz_heif_primary_color_facts`) and packs such a file through
  `rgba16_le_left12` / `rgba16_le_opaque_left12`, which widen RGB `(c << 4) | (c >> 8)` (alpha never), refusing a
  sample with low bits set; every other file keeps its token and its bytes. Gated by D6's `avif12`, `avif12_alpha`
  and `avif12_lossy_identity` (widened, exact against `avifdec`) and `avif12_lossy`, `avif12_lossy_alpha` and
  `avif12_premultiplied` (unchanged, exact against the oracle's `raw16`). A premultiplied AVIF still loses its alpha
  in ImageIO. 12-bit 4:4:4 HEIC stays refused.
- `DONE` — host-still admission is measured per depth, not a
  flat 64 B/px. A still is affine per depth class, `F + a·px` (+ 3·encoded + 64 MiB) through
  `HostSourceInput.scratch`: 8-bit F = 187 MiB, a = 18 (48 MP charge 1,073,963,008, 22.0 B/px), covering every
  measured case at 1.25× its peak at 12, 48 and 96 MP and each kind's 48→96 MP line continued to the 100 MP
  limit (that continuation also extrapolates each kind's encoded size, and so its `3·encoded` credit). The
  measured cases are synthetic gradient-plus-noise fixtures, the system iMac Blue and Sonoma HEICs (36 MP), and the system
  iMac Blue and Sonoma desktop pictures re-encoded to 8-bit HEIC at 36, 48 and 96 MP (Sonoma also with a gain map);
  no camera original was measured. Real content at 48 and 96 MP peaks below the synthetic HEICs; the binding case is
  the 36 MP iMac Blue re-encode (margin 435,305), whose smaller file earns less encoded credit than the original;
  the Sonoma original is next (margin 1,321,186, charged 1.2517× its capture peak).
  Wide F = 17 MiB, a = 29 (48 MP 1,432,186,880; a is the 1.25× requirement's 12→48 MP slope rounded up, since 96 MP
  wide stills exceed the budget and cannot be measured). 48–~71 MP wide is extrapolated at a = 29 (≥ the measured
  28.3 requirement slope); above ~71 MP the estimate alone exceeds the default budget, so capture is refused. The
  decoder-buffer floors (12 / 24 B/px) hold at every size. A per-pixel-only figure needed 31 B/px because the
  12 MP cases carry a fixed overhead, and refused a 48 MP replace. 96 MP 8-bit opens were refused by the core
  ("precision operation needs … including retained inputs"); re-measured at bff4261 (batch 163) at the 2 GiB
  reference limit, this estimate refuses first: heic8-96 "needs 2.01 GB … on its own", gain-map HEIC and AVIF
  the generic working-memory sentence, while 96 MP JPEG and PNG open. Since batch 164 a single 8-bit still's
  WHOLE open is admitted up front
  (`HostStillDecoder.openScratch` = 30 MiB + 41·px + 3·encoded + 64 MiB, fitted by the same rule to MCP
  `open_document` peaks at 12/48/96 MP and the 36 MP system HEICs, record beside the constants): the first
  projection (`rz_doc_flattened`, 20 B/px over the ≈ 1.22 GB document) is the open's peak, about 3.13 GB at 96 MP.
  96 MP heic8 / gain map / AVIF open at 8 GiB at 1.340× / 1.339× / 1.312× their peaks and are refused at the 2 GiB
  reference limit and on an 8 GB Mac ("This job needs 3.91 GB of working memory on its own…" for the HEIC). The
  48 MP HEIC opens (need 2,180,368,909 / 2,176,287,400) now exceed the 2 GiB reference limit and fit the
  2,862,612,480 an 8 GB Mac gets, so verify.py's A1/A2 run there; a 48 MP AVIF and the 36 MP system HEICs stay
  within 2 GiB. Since batch 165 the core's flat JPEG/PNG open is admitted whole too
  (`FlatOpenAdmission.openScratch` = 34·px +
  3·encoded, same rule, ≥ 1.26× at 12/48/96 MP, record beside the constants): 48 MP JPEG / PNG (1,721,414,325 /
  1,967,568,597) open at 2 GiB (A7, and A7-plan refuses the JPEG at 1 GiB), and 96 MP is refused at 2 GiB and on
  an 8 GB Mac ("This job needs 3.16 GB / 3.61 GB … on its own"). Since batch 166 every other 8-bit open (TIFF,
  WebP, GIF, BMP, PSD, `.rz`, a 16-bit PNG at 8 bits, the ImageIO fallback) admits its first projection after the
  decode (`FlatOpenAdmission.firstProjectionBytes`),
  so a 48 MP TIFF needs 2.23 GB on its own and is refused at 2 GiB. Plain PNG/JPEG/TIFF placement is
  `ImageSourceFile.scratchPerPixel8` = 10 and `scratchPerPixel16` = 19 (measured; was 40). The records sit beside
  the constants. `scripts/host-still-admission/verify.py` pins 48 MP 8-bit HEIC, gain-map HEIC and AVIF opens, the
  iMac Blue HEIC, place/linked place/relink into a 12 MP document, a 48 MP JPEG/PNG open and a 48 MP JPEG place into
  a 48 MP document, and a replace with a 48 MP gain-map HEIC in a document already holding two 48 MP sources.
  A multi-image still (Sonoma's light/dark pair) opens its primary image, with that image's metadata, PPI and
  mattes (gate A9); each host-still refusal names its reason, and an over-budget decode names its working memory.
  Not covered: 48 MP 10-bit stills are refused by the core's 16-bit save-and-reopen admission.
- `DONE` — a typed 16-bit document's `get_document` reports
  `save_preservation` through the byte builder (`AgentServer.preservationFields`), so the typed HEIC depth sentence
  from `PrecisionExportCapture.losses` has its MCP surface, gated by the drive's D1. A float document reports it
  too: its flat 16-bit `save_copy` / Save To tone-maps through the export workspace's float stage
  (a remembered 32-bit export depth falls back to 16), with float-worded losses (11F's F11).
- `OPEN` — multipage (Part B) and vector-preserving (Part C) PDF export (no ledger owner).
  Part A is done: A0 enabled File ▸ Export…, Quick Export, Export Layers… and Contact Sheet… at 16 bits (Export…
  and Quick Export at 32-bit float too); A1 writes Export as PDF… / `export_pdf` at 8 or 16 bits per component from
  a 16-bit or float document (a float page is its tone-mapped projection); A2 adds a JPEG-compressed 8-bit page.
- `DONE` — float TIFF in the app (11F, `a3e4f2c`); general PPI editing (P7-26, 12E); typed auxiliary mattes (9G,
  P7-08–P7-11 verified).
- `DROPPED` — HEIC IPTC (host encoder drops it; disclosed); SVG features CoreSVG doesn't draw (disclosed, not
  detected).
