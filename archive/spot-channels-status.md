# Spot channels (#547): status

- **Status:** Open — 1a-1 (core model, overprint kernel, `.rz` word 112), 1a-2 (host half), 1b-1 (PSD/PSB spot
  channels in and out, Photoshop-authored fixtures), 1b-2 (Photoshop's readback take; 1b complete), 1c-1 (Merge
  Spot Channel, alpha ↔ spot, the typed exports' and the region render's overprint) and 1c-2a (flat TIFF spot
  channels in and out, Photoshop-authored fixtures; PDF measured and kept refused) shipped; 1c-2b to come
- **Issue:** [#547](https://github.com/kpgalligan/doctored-dev/issues/547) (the colour-mode umbrella; spot channels are
  part of its rest, `designs/cmyk-mode-status.md`, and Multichannel waits on them, `designs/multichannel-mode.md`)
- **Code:** `core/src/spot_channel.rs` (model and adapters), `core/src/spot_overprint.rs` (KERNEL: the overprint),
  `core/src/spot_channel_native.rs` (`.rz` word 112 and `SPOT`), `core/src/spot_view.rs` and
  `core/src/ffi_spot_channel.rs` (the ink's encoding, the overprint of a document, the exports),
  `core/src/psd_spot_records.rs` (KERNEL) and `core/src/psd_spot.rs` (PSD/PSB), `core/src/spot_typed.rs`,
  `spot_cmyk.rs`, `spot_merge.rs` and `ffi_spot_merge.rs` (1c-1), `core/src/tiff_spot_samples.rs` (KERNEL) and
  `core/src/tiff_spot.rs` (1c-2a); tests `core/tests/spot_channels.rs`, `spot_channel_ffi.rs`, `spot_psd.rs`,
  `spot_psd_photoshop.rs`, `spot_merge.rs`, `spot_typed_export.rs`, `spot_tiff.rs`, `spot_tiff_photoshop.rs`; host
  `app/Sources/SpotChannel.swift` and its siblings (`app/FILES.md`); drive `scripts/verify_spot_channels.py`;
  admission `scripts/spot-admission/measure.py`

## Sources

1b-1's PSD/PSB layout is measured (`scripts/photoshop-probe/spot_psd_take.py` take1, recorded in its docstring
and `core/tests/fixtures/psd_photoshop/spot_547/README`), and 1c-2a's flat TIFF layout and PDF form
(`scripts/photoshop-probe/spot_tiff_take.py` take2, its docstring and
`core/tests/fixtures/photoshop_spot_tiff/README`). Everything else is as the 1a request states it,
unmeasured; nothing fetched:

- **New Spot Channel** (Channels panel menu): Name, and Ink Characteristics: Color and Solidity, 0–100 %.
- **Spot Channel Options**: the same fields on an existing spot channel.
- **Solidity** is the ink's opacity on screen: 0 % a transparent ink that shows the colours beneath, 100 % an ink
  that covers them (it is display and composite only, never the printed ink).
  New Spot Channel's defaults are red at 0 %.
- The composite shows each spot channel's ink overprinted on the image; it is a preview of the printed
  result, and the layers are unchanged.
- **Merge Spot Channel** folds a spot channel into the colour channels.
- Spot channels exist in RGB, CMYK, Grayscale, Lab and Duotone, not in Bitmap or Indexed Color.
- Multichannel mode, whose channels would be spot colour channels, is not built (`designs/multichannel-mode.md`).

## Decisions in force

- **A spot channel is a channel with `spot` set**: the byte `Channel.spot`, the typed `ChannelMetadata.spot`
  (`Option<SpotInk>`). It shares the alpha channel's storage and everything built on it: the `MAX_CHANNELS` count
  cap and the channel pixel budget, the geometry ops, duplicate, delete and load-as-selection.
- **The ink is D50 CIE Lab in f64** (Photoshop's descriptor stores a spot colour as Lab; Duotone inks are held the
  same way, and `duotone_ink`'s range rule is the one check), plus **Solidity as an integer percent**, 0–100.
- **The plane is ink coverage** at the document's channel depth: 255 (byte) or 65535 (every typed document) is
  100 % ink.
- **Modes:** admitted on byte RGB and on typed RGB 16/32, CMYK, Gray, Lab and Duotone (32-bit admitted, not
  verified); Indexed and Bitmap refuse by name ("Spot channels are not available in Indexed Color mode." /
  "… in Bitmap mode …"). Converting into Indexed or Bitmap refuses while a spot channel exists ("Indexed Color
  has no spot channels; delete the spot channels first." / "Bitmap has no …"), in the plan's `refusal` and the
  conversion alike; every other mode and depth conversion carries the ink.
- **The overprint** (`spot_overprint`, Doctored's reading, unmeasured; re-read in 1a-2 so a default 0 % ink
  shows): the ink colour source-over the composite at weight coverage / max, with the blend Solidity `s` mixes
  from Multiply toward the ink, `B = (1 − s)·Multiply(Cb, ink) + s·ink`, on encoded values, one spot channel at a
  time in channel-list order. Solidity 0 is Multiply at coverage, 100 Normal at coverage. Display only; it never
  changes a layer. A coverage of 0 leaves the pixel exactly as it was, at any solidity.
- **The ink's encoding** (`spot_view::ink_encoded`): the Lab twin through `ColorPipeline` (relative colorimetric,
  no BPC) into the composite's profile; a profile the pipeline refuses as a destination reads as the built-in sRGB.
- **Saves:** `.rz` keeps spot channels. The PSD/PSB refusal follows the writer's own rule,
  `psd_export_preflight::spots_written_for`: RGB, CMYK, Gray and Duotone PSD/PSB at 8 and 16 bits save their spot
  channels (a 16-bit Duotone document's file is written at 8 bits, its spots with it), asked with the document's
  real depth by the preflight, the report and `save_psd` alike (the typed save through `spot_sample_bytes`); a Lab
  or 32-bit PSD/PSB and the layered TIFF refuse (`PSD_REFUSAL`, the report and `save_psd`). Since 1c-2a a flat
  TIFF SAVE keeps them (the same rule's `SpotFile::FlatTiff`: RGB and CMYK at 8 and 16 bits; a Grayscale, Duotone,
  Lab or 32-bit document refuses with `TIFF_REFUSAL`), and PDF refuses in the host (`pdfRefusal`, the measured
  sentence). Every other flattened save or export, Export As TIFF included, composites the overprint and says so
  (`flat_save_losses`' spot line, the `spot_channels` reply key): an 8-bit RGB one in the host
  (`flatExportProjection`), every typed one in the core (1c-1, `spot_typed`), at every depth and mode.
- **Flat TIFF (1c-2a, measured by `scripts/photoshop-probe/spot_tiff_take.py` take2, Photoshop 26.10.0):** a spot
  channel is a TRAILING extra sample after the colour samples and any transparency, ExtraSamples 0, chunky; its
  name and ink are the PSD's 1006/1045/1077/1053 in tag 34377, every list leading with "Transparency" when the
  file has one (L1); its samples are stored `max − coverage` in RGB and CMYK alike (H1, `tiff_spot_samples::
  tiff_stored`, through `psd_spot_records::stored`); the composite holds no overprint (C0). Doctored reads every
  lossless compression (Photometric laid to 1 through `tiff_directory::Overlay`, one Multiband decode) and keeps
  kind-2 Lab spots by the PSD reader's own rules and sentences (`psd_spot::spot_candidates`); planar samples, an
  Orientation other than 1 and an unread compression are a loss line with nothing kept. It writes uncompressed
  chunky strips of at most 8 MiB, the plain composite, a transparency only when some pixel is not opaque (as
  ExtraSamples 2, straight; Photoshop writes 1) and CMYK never, then the spots, with their resources after the
  filtered inherited run (`DROPPED_8BIM` drops 1006, 1007, 1045, 1053 and 1077 from every save). The byte save
  (`save_image`), the typed save (`ExportRequest` `"spot_channels":"keep"`, TIFF at full size only; a CMYK
  document refuses it, its save keeps them) and the CMYK save (`cmyk_encoded`'s TIFF arm) write it; a 16-bit RGB
  TIFF with extra samples opens only when every one is a kept spot channel (`tiff_spot_extras_accounted`).
- **PDF (1c-2a):** Photoshop's PDF keeps spot channels as `/Separation` colourants (PDF-FORM SEPARATIONS on both
  take2 PDFs), which Doctored's PDF cannot write, so `pdfRefusal` stays ("Spot channels are not saved to PDF:
  Photoshop's PDF keeps them as separate inks, which Doctored's PDF cannot write; save as TIFF, PSD or .doctored,
  or delete the spot channels first."); Doctored's PDF open rasterizes the page, so no spot channel is read.
- **The typed overprint** (1c-1, `spot_typed`): each spot channel in list order, coverage its own u16 plane, through
  the one kernel. RGB 16/32: the ink encoded under the composite's profile (`ink_encoded`), `overprint_rgba16` or
  `overprint_rgba32f` (unclamped, so a float composite keeps its range above white). Duotone: the RGB rule on the
  Duotone RGB view, under the view's profile. Gray: the ink's L* alone, Lab → Grayscale's take-confirmed rule
  (`lab_gray`), written R = G = B. Lab: through the built-in ProPhoto RGB (Lab colour ops' space, relative, no BPC)
  and back at the document's depth, the converted-back sample taken ONLY where some listed coverage is above 0, so
  a coverage-0 pixel keeps its exact Lab codes. The flattened export hook is `export_encoded` (`spot_export_stages`);
  `export_plan` adds `spot_overprint_scratch_bytes`, and a layer export never overprints.
- **CMYK overprints in ink on paper** (1c-1, `spot_cmyk`): a spot ink overprints CMYK as ink, so the
  press separations carry it, and multiplying through the display would hand a press an RGB-derived ink. The
  ink's CMYK is its D50 Lab → PCS XYZ → `ToDevice` from the Lab twin into the document's CMYK profile (relative, no
  BPC), clamped, and a profile with no B2A table refuses the merge and those exports by name rather than print
  no ink; the kernel's CMYK arm runs the one body twice a pixel on `L = 1 − ink` ((C, M, Y) then (K, K, K)).
  The converted export, the kept export (always its staged branch) and the JPEG-intermediate save take the
  overprinted on-paper composite; the CMYK TIFF save and every PSD keep `cmyk_flat_samples` with the spot planes
  as channels (the TIFF's since 1c-2a).
- **Merge Spot Channel** (1c-1, `spot_merge`) flattens first, with no prompt and with Flatten
  Image's rules (hidden layers discarded, comps cleared, every other channel kept), bakes the ONE ink through the
  kernel the view uses, and deletes the channel: one result, one undo step. A Duotone document refuses
  (`DUOTONE_MERGE_REFUSAL`). The typed plan is the flatten's charge plus the overprint's term; in CMYK the
  flatten's charge alone, since the overprint fills the flatten's own on-paper plane in place.
- **Alpha ↔ spot** (1c-1): a channel keeps its picture when its kind changes, and black is 100 % ink on a
  spot channel. Doctored stores an alpha channel as selection (255 = selected, white) and a spot plane as ink, so
  both directions invert the plane, `max − v`. Polarity is not read; spot → alpha leaves the rubylith fields as
  they were. Channel Options' third "Color Indicates" choice, Spot Color, takes the colour as the ink and the
  opacity as Solidity; Spot Channel Options' Kind: Alpha Channel turns it back; MCP `set_channel_kind`, whose new
  ink defaults to the UI's. No `.rz` change: `SPOT` marks spots.
- **The composite view** overprints each VISIBLE spot channel over the displayed composite (a spot eye is on
  unless hidden; a spot channel draws no wash), and draws one ink dark when it alone is the base. A live tick
  drops the overprint base to the washes alone. `render` overprints its full canvas, and since 1c-1 a `region` or
  `preview_divisor` render overprints the samples the full render's crop holds (output (i, j) is canvas
  (x + i·d, y + j·d), `regional`'s rule; `spot_view::image_spot_overprint_region`).
- **`.rz`:** conditional word 112 at both depths (byte: the first word inside 45; typed: right after the CMYK
  slot) and its `SPOT` table after `LREF` (u32 schema 1, u32 count, then u32 index ascending, f64 L, a, b, u8
  solidity). Written only when some channel is spot, so every other document's bytes are the prior writer's;
  `DRDC_VERSION` stays 89. The typed reader applies `SPOT` after CHPR (which replaces each channel's metadata) and
  refuses `SPOT` in an Indexed or Bitmap file. A spot-only channel writes no CHPR.
- **PSD/PSB (1b-1, measured by `scripts/photoshop-probe/spot_psd_take.py` take1, Photoshop 26.10.0):** a saved
  channel is a spot channel when its 1077 DisplayInfo kind is 2; its ink is colour space 7 (Lab: L, a, b × 100,
  a and b signed) and its opacity is Solidity. The plane is stored INVERTED, `max − coverage` (H1 on all seven
  files); `psd_spot_records::stored` is the one map, both directions. Resources 1006 (Pascal; Photoshop's is Mac
  Roman, ours the UTF-8 name, both readers prefer 1045), 1045 (count includes one trailing NUL), 1077 (version
  1, 13-byte records) and 1053 (ids from 3); no 1007. With a negative layer count every list leads with the
  merged transparency's "Transparency" entry (kind 1, space 0, id 0); the reader sets it aside and the writer
  writes it whenever its count is negative. The first saved plane follows the colour planes and merged alpha.
  **Read back (1b-2, `scripts/photoshop-probe/spot_psd_readback_take.py` take1 and take2, 26.10.0):** Photoshop
  opens Doctored's RGB, CMYK and Gray files at 8 and 16 bits and Duotone at 8, PSD and PSB, merged transparency
  and the non-ASCII name included, with every spot channel's name, order, Lab, Solidity and coverage as written; it
  opens no 16-bit Duotone file, with or without spot channels. So a 16-bit Duotone document's PSD/PSB is written at
  8 bits (`psd_export_gray`; the Duotone ADR), not refused, and take2 confirmed Photoshop opens those files.
- **What is read and written:** RGB, CMYK, Grayscale and Duotone at 8 and 16 bits, PSD and PSB, both import
  choices; a 16-bit Duotone document is written at 8 bits. Alpha (non-spot) channels stay a disclosed loss both
  ways. A header holds at most 56 channels
  (`psd_framing::MAX_PSD_CHANNELS`): the writer writes the first `56 − composite planes` spot channels and names
  the rest. Lab, 32-bit and layered-TIFF exports write none (Lab and 32-bit name them in their own loss; a TIFF
  counts them in the saved-channels loss); a ZIP composite's spot planes are disclosed, not read; an unreadable
  spot plane, a resource mismatch, a non-Lab ink and an ink past `SpotInk::new` are loss lines, never a refused
  open, and a layered open still succeeds.

## Plan

1. **1a-1** (shipped): the model, the overprint kernel, `.rz` at both depths, the conversion refusals, core tests.
2. **1a-2** (shipped): FFI, Swift, the composite view's overprint, the Channels menu and sheet, MCP, save
   refusals, the drive.
3. **1b** (shipped): PSD/PSB import and export with Photoshop-authored fixtures and a take: **1b-1** the author
   take, the fixtures, the reader and the writer; **1b-2** the readback take.
4. **1c**: **1c-1** (shipped) Merge Spot Channel, alpha ↔ spot through Channel Options, the typed flattened exports'
   and the region render's overprint; **1c-2** the TIFF and PDF paths, with Photoshop-authored fixtures, in two
   slices: **1c-2a** (shipped) the author take, flat TIFF in and out, PDF per the take; **1c-2b** the readback take
   and the layered TIFF.
5. Then Multichannel's reopening, an owner decision (`designs/multichannel-mode.md`).

## Contracts of the sub-slices not yet shipped

- **1c-2b.** (1) A READBACK take: Photoshop opens Doctored's flat RGB and CMYK TIFFs at 8 and 16 bits with spot
  channels (one with a transparency and the non-ASCII name), judged against bounds declared in the runner's
  docstring before it (opened in its mode and depth; every spot's name, order, kind, Lab within 0.5, Solidity
  exact, coverage: the block exact, the ramp within 1 / 4 codes), a take under the binding Photoshop rules. (2) The
  layered TIFF reader and writer from 1c-2a's layered fixtures (`rgb8_l.tif`, `cmyk16_l.tif` in
  `core/tests/fixtures/photoshop_spot_tiff`: the composite's trailing spot extras as in a flat file, ExtraSamples
  `[1, 0, …]`, tag 37724 `Layr`/`Lr16`), lifting the layered-TIFF `PSD_REFUSAL` only if their rule can be met.
  (3) A PDF reader only if a raster reader can decode the take's `/Separation` image XObjects; otherwise PDF stays
  out with 1c-2a's measured reason. Until it ships, the layered-TIFF `PSD_REFUSAL` and `pdfRefusal` stand.

## Open items

- The overprint rule is Doctored's own reading, not measured against Photoshop
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- 32-bit documents admit spot channels without verification of the display path; the export's overprint is
  tested (`spot_typed`'s f32 arm, `export-float-exr32`) ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- Merge Spot Channel and the alpha ↔ spot kind change are declared, not measured
  against Photoshop; a take can measure them later ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
- A 16-bit Duotone PSD/PSB export's 8-bit copy is admitted at the depth step's own plan (`gray_profile_plan`, which
  includes a display term an export copy does not use): 1.68 GB planned against 65 MB measured at 24 MP
  (`scripts/duotone-admission/measure.py`, `export-duotone-psd16`). It passes; it is over-admission, kept by the
  controller's ruling ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).

## Shipped

- **1a-1** (commit 051d3cbe): `SpotInk` and the byte/typed add and set (`spot_channel.rs`), the overprint kernel
  (`spot_overprint.rs`), `.rz` word 112 and `SPOT` (`spot_channel_native.rs`), the Indexed/Bitmap refusals in
  `indexed_convert`, `indexed_document` and `bitmap_convert`, and the carry through duplicate, clone, promote and
  narrow.
- **1a-2**: the kernel re-read (weight = coverage, Solidity mixes Multiply toward Normal);
  `spot_view.rs` and the seven `ffi_spot_channel.rs` exports with their header section and wrappers
  (`DoctoredCore+SpotChannels.swift`); the info row's `spot`; Channels ▸ New Spot Channel… and Spot Channel
  Options… (`SpotChannelSheetController`), the panel row (ink badge, "spot · N% solidity", ink-dark thumbnail,
  eyes on unless hidden, `ChannelVisibility.hiddenSpots`), the composite view's overprint; the save refusals and
  the flattened byte overprint; MCP `add_spot_channel` / `set_spot_channel` and `spot` in the channel rows. The
  drive `scripts/verify_spot_channels.py` S1–S8 measures the PNG and the render at 0 codes from the oracle (black
  and red inks, default ink).
- **1b-1**: `psd_spot_records.rs` (KERNEL: 1006/1045/1077/1053 and the polarity) and `psd_spot.rs` (the
  container read, `with_psd_spots` at every byte, typed, CMYK and Gray open site, `psd_spot_planes` for every
  8/16-bit PSD writer); `write_psd` declares the spot planes after the composite; the preflight's `spot_channels`,
  worst-case term, losses and observation. Fixtures `core/tests/fixtures/psd_photoshop/spot_547` (seven files,
  `photoshop.json`); tests `core/tests/spot_psd.rs`, `core/tests/spot_psd_photoshop.rs`. No FFI, no `.rz` change.
  At integration with 1a-2, the PSD/PSB save refusal was lifted where `spots_written` holds (RGB, CMYK, Gray and
  Duotone at 8 and 16 bits); Lab, 32-bit, TIFF and PDF still refuse.
- **1b-2**: `scripts/photoshop-probe/spot_psd_readback_take.py` (the jsx's `spot_psd_readback`), take root
  `spot-psd-readback-547`, Photoshop 26.10.0. take1 PASSed fifteen of seventeen gated files and MISSed
  `duo16.psd`/`.psb` on opened (the spot-free control too). By RULING-1, a 16-bit Duotone document's PSD/PSB is
  written at 8 bits from the depth op's copy, with one loss line and the report's depth 8 (`psd_export_gray`,
  `PSDExport.bit_depth`); `spots_refused` asks the rule with the document's real depth (`spot_sample_bytes`).
  take2 (run `20261025T152228Z-63b5d535`, the same rows at file depth 8) PASS: both files open as Duotone at 8
  bits with every row held, and Photoshop's copies' planes and 1077 records equal ours. The record tests in
  `spot_psd_photoshop.rs` read both takes and SKIP when absent.
- **1c-1**: the kernel's f32 and CMYKA16 arms over the one body; `spot_view`'s region sampling;
  `spot_typed` (per-mode overprint, its plan term, the export hook), `spot_cmyk` (the ink's CMYK, the on-paper
  composite with spots), `spot_merge` (byte and typed merge and kind change) and the seven `ffi_spot_merge`
  exports with their header block and wrappers. `typedExportRefusal` and `regionNote` are gone; the typed flat
  copy takes `PrecisionExportCapture`'s request branch and names `FLAT_LOSS`. Channels ▸ Merge Spot Channel (menu
  and spot row), Channel Options' Spot Color, Spot Channel Options' Kind; MCP `merge_spot_channel`,
  `set_channel_kind`. Tests `core/tests/spot_merge.rs`, `spot_typed_export.rs`; the drive's S5/S7 updated and
  S9–S11 added; `scripts/spot-admission/measure.py` holds the six new plan terms (receipt in the handoff). 1c-2
  must lift `tiffRefusal`/`pdfRefusal` and give the CMYK TIFF save its spot path (`cmyk_io::cmyk_encoded`'s `Tiff`
  arm stays on `cmyk_flat_samples` until then).
- **1c-2a**: `scripts/photoshop-probe/spot_tiff_take.py` (the jsx's `spot_tiff` section), take root
  `spot-tiff-547`, Photoshop 26.10.0: smoke1 CLEAN; take1 skipped before anything was driven (a Photoshop the run
  had not launched was open; spent by ruling); take2 (run `20261026T230703Z-a8b8762a`) MEASURED, fixtures
  `core/tests/fixtures/photoshop_spot_tiff` (seven TIFFs, `photoshop.json`). `tiff_spot_samples.rs` (KERNEL) and
  `tiff_spot.rs`; `psd_container::resource_in`, `psd_spot::spot_candidates`; `SpotFile` in `spots_written_for`;
  `TIFF_REFUSAL`; `TiffSpotPlanes` and `EncodeSidecar.spots`; `ExportRequest` `spot_channels`. Host:
  `tiffRefusal`/`exportRefusal` gone, `flatSaveProjection`, `tiffSaveScratch`, `keepSpotChannels`, the measured
  `pdfRefusal`; MCP `save_copy tif` keeps them (a CMYK document's through its save) and replies
  `working_memory_plan_bytes`. Tests `spot_tiff.rs`, `spot_tiff_photoshop.rs`; drive S6 updated, S13 added;
  `scripts/spot-admission/measure.py` holds the five TIFF plan terms (24 MP, Release, 3 runs, every case PASS;
  receipt in the handoff). No FFI, no `.doctored` change.
- **`convert_precision` keeps the channels**: the flattened 8→16, 16→8 and 32→8 conversions carry every alpha and
  spot channel (×257 / nearest, the selection masks' rounding); every other step already did; no step discards
  them; the flatten promotion's plan now counts them, as preserve does; drive S12
  ([#547](https://github.com/kpgalligan/doctored-dev/issues/547)).
