# HDR photos and the EDR canvas (v1.x ADR)

- **Status:** Accepted, 2026-10-13. Ships opening HDR photos (`hdr-photo-open`) and the EDR display. RAW
  HDR output followed (2026-10-20: tone curve and Shadows at float, Develop's HDR preview).
- **Code:** core `hdr_transfer.rs`, `float_host_intake.rs`, `float_view.rs` (`display_extended`),
  `display_encoding.rs` (rule X), `doc_export_float.rs` (the HDR intermediates); app `HDRPhotoRequest.swift`,
  `HDRStillDecoder.swift`, `HDRStillOpen.swift`, `DoctoredCore+HDR.swift`, `EDRDisplay.swift`,
  `EditorViewController+HDRPreview.swift`, `ImageCanvasView+ExtendedRange.swift`, `HostImageEncoder.encodeHDR`.
- **Checks:** `make t F=hdr_transfer`, `F=float_host_intake`, `F=float_display_extended`, `F=export_hdr_photo`;
  `scripts/verify_v1x_hdr_photos.py` (fixtures by `scripts/hdr-photos/author.py`); the jobs-probe cases
  "hdr open choice", "hdr revert keeps rendition", "hdr preview toggle", "hdr export rows" (`DR_EDR_HEADROOM=4`).

## Decisions in force

- **One reference-white rule.** 1.0 in a float document is SDR reference white: a gain map's SDR base white,
  and 203 cd/m² for PQ and HLG (BT.2408; HLG through the BT.2100 OOTF at Lw 1000, γ 1.2, Ys from the stated
  primaries, so a 75 % HLG grey is 1.0). The math that is ours is `hdr_transfer.rs`; the gain-map math is Core
  Image's, checked against ISO 21496-1 written independently in the drive.
- **The choice is an open-as-HDR option.** File ▸ Open HDR Photos as HDR (UserDefaults
  `OpenHDRPhotosAsHDR`, default on) and MCP `open_document hdr_photo: "hdr"|"sdr"`. HDR opens a 32-bit float
  document; SDR opens a gain map's SDR base (today's host path, disclosed) and refuses a PQ/HLG still by name (it
  has no SDR rendition). `precision: "rgba8"` takes the SDR route. Revert reopens the rendition the document was
  opened with (`ImageDocument.hdrRendition`, never saved). Load Files into Stack and Merge to HDR scope `.sdr`;
  Place, linked images and the retained-image tools keep `HostStillDecoder`'s SDR base / PQ refusal.
- **Classification and decode (host).** An ISO or Apple gain-map auxiliary at the primary image is a gain map;
  otherwise a lazily created `kCGImageSourceDecodeToHDR` image whose space uses a BT.2100 transfer is PQ/HLG by
  name (`itur_2100`, `itur_2020`, `displayP3`, and `itur_709`, which ImageIO names a BT.709 cICP/nclx still).
  A gain map is `CIImage(contentsOf:options:[.expandToHDR: true])` rendered as premultiplied `.RGBAf` in the SDR
  base space's extended-linearized twin, whose ICC becomes the document profile; refused by name at headroom ≤ 1.
  PQ/HLG is ImageIO's integer HDR decode through `HostStillDecoder.layout`. Orientation goes to the core.
- **Core intake.** `dr_precision_doc_from_host_hdr_pixels`: `linear` takes `rgba32f_*` and a matrix/TRC ICC;
  `pq`/`hlg` take a 16-bit or `rgb10_packed_le` layout and `bt709`/`display_p3`/`bt2020` primaries (BT.2020
  converts once in linear light to sRGB primaries with `float_io::profile_of`'s warning, negatives kept). Built
  through `float_io::document_of` (`FloatContainer::HostStill`), so `raster_float` stays the one F32 leaf
  constructor. A non-finite f32 sample or one past ±65504 refuses by name. Plan: input + the f32 sample vector
  and the tiles (32 B/px) + bookkeeping, admitted before allocating.
- **Export.** `hdr_encoding` (`gain_map` | `pq`, only with `bit_depth: 32` and `heic`/`avif`) makes a pending
  host-encoded result; `info_json` adds `hdr_encoding` and `content_headroom` (max(1, the largest linear value
  written)). The gain-map intermediate is the `tiff` 32-bit bytes and is written by **Core Image** with a colour
  gain map; ImageIO's `EncodeToISOGainmap` was measured and rejected (a monochrome map that ignores its
  pixel-format request, and a reconstruction capped at 0.8 × the declared headroom). PQ is a 16-bit TIFF of
  BT.2100 PQ codes in BT.2020 labelled `itur_2100_PQ`, written by ImageIO's `EncodeToISOHDR`. The written file is
  read back and must carry an ISO gain map / a PQ space. AVIF refuses by name (Core Image writes none; ImageIO's
  HDR AVIF encode fails on macOS 26.5.1); the export sheet disables the HDR row for AVIF (the refusal is its
  tooltip) and runs no preview encode for it. `save_copy` writes no HEIC/AVIF at any depth, so HDR export is
  `export_image`'s.
- **EDR display (float documents only).** Rule X (`display_encoding.rs`): `clamp(v·2^stops, ±65504)·α` and α as
  binary16, no curve and no tone map; the host wraps the buffer in the document space's extended-linearized
  twin. HDR Preview (per window, default on) engages when the window's screen's potential headroom is above 1
  (`DR_EDR_HEADROOM` overrides for tests), the document is float and no candidate tone map is pending. Engaged:
  the canvas layer is `RGBA16Float`, `wantsExtendedDynamicRangeContent`, `toneMapMode .never` (and
  `preferredDynamicRange .high` on macOS 26), and the window's colour space is extended Display P3 (the prior
  one restored on the way back). An extended render refusal falls back to the SDR view. Info, samplers,
  `sample_pixel`, `render`, thumbnails, the Navigator and save read document values only.
- **Develop's HDR preview (#4).** A float development, potential headroom > 1, Fit or 100 %, Clipping
  on or off: each f32 side draws `develop_preview_extended` of its region (the SDR preview's cells, centres
  through rule X at gain 1) from its retained f32 master in its profile's extended-linearized space, the canvas
  layer set by the one `EDRDisplay.configure` and the window in extended Display P3 while it shows. Clipping marks
  a cell holding a sample at or below 0.0 / at or above 1.0 (`clipped_channels_float`, the ⌥ display's rule) in
  opaque binary16 blue/red/magenta. Without headroom every view is the SDR-clipped one, as before. The histogram
  reads the f32 master through the one float scoped histogram (stops −12…+6 EV, above-white counts) at any
  headroom. Its plan is the EDR view's at the preview size (≤ 1600 per side); a viewport-only frame charges the
  session's own `extendedFramePerPixel` (16 B) per output pixel of each f32 side beside the fixed 48 MiB (VF32,
  Release: max E 12.0 B per output pixel at Fit and at 1024² 100 % on the NEF, D24 and D850). MCP `inspect_raw_development` reports `hdr_preview` (following `region` and `clipping`) and the
  float histograms.
- **The canvas draw is a display list.** AppKit records a layer-backed view's `draw(_:)` as
  `kCGContextTypeDisplayList` (no bitmap) that Core Animation rasterizes into the 16-bit float backing store,
  so `ExtendedDrawRecord` reads depth 0 there; the jobs-probe renders the layer into a float extended bitmap
  (`CALayer.render(in:)`, which runs `draw(_:)` with that bitmap) and reads the drawn 4.0 back.

## Fixtures and oracles

`author.py` writes authored flat-patch scenes only (no photo) into `~/devel/doctored-fixtures/hdr-photos/`:
ImageIO's ISO gain map (greys only — its monochrome map moves chroma), Core Image's `.hdrImage` and
`kCIImageRepresentationHDRGainMapImage` (Apple auxiliary only), libavif's `combine` (with an explicit base
matrix: ImageIO misreads a gain-map AVIF whose base declares matrix 2), PQ and HLG HEIC (HLG from BT.2100 codes
through Core Image's writer in the HLG space itself: Core Image's HLG model of a LINEAR scene is not BT.2100's
luminance OOTF for a saturated colour, measured 4.5 % dark on (2.5, 1.0, 0.3)), PQ/HLG AVIF (`avifenc --cicp`),
PNG with cICP, and PQ JPEG XL (`cjxl`, which ImageIO decodes as `ITUR_709_PQ`).

## Measured admission

Release app, `scripts/verify_v1x_hdr_photos.py --measure 3` (8 GiB, `MallocLargeCache=0`, one case per fresh
instance, a baseline twin per run): the records are beside the constants.

- **The open** (`HDRStillDecoder.openScratch`, `64 MiB + 64·px + 3·encoded + 64 MiB`): max E 632,586,696 /
  2,564,818,888 (gain map 12 / 48 MP), 590,037,472 / 2,352,383,632 (PQ); plan 1.4458× / 1.2694× / 1.5501× /
  1.3840×. A 48 MP HDR photo needs 3.26 GB.
- **The EDR view** (`float_view::extended_display_scratch`, `1 MiB + 46·px`): the buffer-only plan (8 B/px)
  missed by 4.4× — the window's CGImage and Core Animation's converted copy bring the peak to 33.7–36.6 B/px;
  max E 446,300,496 (PQ 12 MP) and 1,645,888,736 (gain map 48 MP), plan 1.2591× and 1.3637×.
- **The HDR HEIC export** (the existing 32-bit export plan, 96 B/px + 16·width + 130 MiB; `--measure-phases
  export`, the host's Core Image / ImageIO encode included): max E 652,493,208 / 2,944,419,856 (gain map 12 /
  48 MP), 439,877,968 / 1,755,792,632 (PQ); plan 2.003× / 1.6365× / 2.971× / 2.744×, so it stands unraised.

## The real-world case (numbers only)

The owner's Apple ProRAW DNG (in place, read-only, SHA-256 checked; the drive SKIPs without it) opened through
the RAW pipeline at linear float: 5.80 / 5.62 / 5.90 % of its 48,771,072 pixels above 1.0 in R / G / B, maximum
34.31; HDR Preview engaged with an on-screen peak of 34.31, doubling at +1 stop, and turning it off restored the
window's previous view image exactly. ImageIO reports both an Apple and an ISO gain map: Apple's headroom 3.2385
(maker tags 33 = 1.01, 48 = 2.0055 through Apple's published formula; its XMP states 3.23847), the ISO alternate
headroom 2^1.6953 = 3.24, and `kCGImageSourceDecodeToHDR` gives the DNG no content headroom (1.0).

## Open items

- 32-bit PSD read and write shipped ([v1-float-editing.md](v1-float-editing.md), "After v1").
- RAW HDR leftovers (the local tone map at float, no supporting fixture; Visualize HDR, no colour scheme
  on record): [#4](https://github.com/kpgalligan/doctored/issues/4). Grain at float and HDR at 100 % with an
  HDR histogram shipped under it.
- EDR for 8- and 16-bit documents: no such document can carry an HDR (PQ/HLG) profile today (matrix/TRC ICC
  only; `precision_png` refuses cICP).
