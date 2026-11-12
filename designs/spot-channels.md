# Spot channels (ADR)

- **Status:** Accepted — open items remain ([#1](https://github.com/kpgalligan/doctored/issues/1)); the phase closed
  2026-10-26. [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella) stays open (see
  [cmyk-mode-status.md](cmyk-mode-status.md)). Multichannel, which would make spot channels, is closed by decision
  ([multichannel-mode.md](multichannel-mode.md)).
- **Original:** [archive/spot-channels-status.md](../archive/spot-channels-status.md) (the working record at close).
- **Takes:** under `~/devel/doctored-fixtures/photoshop/`: `spot-psd-547` (`spot_psd_take.py`, 1b-1),
  `spot-psd-readback-547` (`spot_psd_readback_take.py`, 1b-2), `spot-tiff-547` (`spot_tiff_take.py`, 1c-2a) and
  `spot-tiff-readback-547` (`spot_tiff_readback_take.py`, 1c-2b); runners in `scripts/photoshop-probe/` in the
  private archive (`kpgalligan/doctored-dev`), each
  docstring holding its rows, bounds and recorded result. Fixtures `core/tests/fixtures/psd_photoshop/spot_547` and
  `core/tests/fixtures/photoshop_spot_tiff`.
- **Code:** `core/src/spot_channel.rs` (model, sentences), `spot_overprint.rs` (KERNEL), `spot_channel_native.rs`
  (`.doctored` word 112, `SPOT`), `spot_view.rs`, `ffi_spot_channel.rs`, `psd_spot_records.rs` (KERNEL),
  `psd_spot.rs`, `spot_typed.rs`, `spot_cmyk.rs`, `spot_merge.rs`, `ffi_spot_merge.rs`, `tiff_spot_samples.rs`
  (KERNEL), `tiff_spot.rs`, `tiff_layers_write.rs`'s `write_spots`; tests `core/tests/spot_*.rs`; host
  `app/Sources/SpotChannel.swift` and its siblings (`app/FILES.md`); drive `scripts/verify_spot_channels.py`
  S1–S16; probe cases "float spot view" and "hdr spot view" (`scripts/jobs-probe/ui_float_spot.swift`);
  admission `scripts/spot-admission/measure.py`.

## Context

Spot channels: Channels ▸ New Spot Channel… and Spot Channel Options… (a name, an ink colour and a
Solidity of 0–100 %), the composite's overprint preview, Merge Spot Channel, an alpha channel's kind changed to
spot and back, and the files that carry them: `.doctored`, PSD and PSB, flat and layered TIFF. They are allowed
in RGB, CMYK, Grayscale, Lab and Duotone, not in Bitmap or Indexed Color. Solidity is display and composite
only, never the printed ink; New Spot Channel's defaults are red at 0 %.

## Decisions in force

- **A spot channel is a channel with `spot` set** (byte `Channel.spot`, typed `ChannelMetadata.spot`). It shares
  the alpha channel's storage and everything built on it: the `MAX_CHANNELS` cap and pixel budget, the geometry
  ops, duplicate, delete, load-as-selection. The plane is ink COVERAGE at the document's channel depth (255 byte,
  65535 typed = 100 % ink). The ink is D50 CIE Lab in f64 (`duotone_ink`'s range rule) plus an integer Solidity.
- **Modes.** Byte RGB and typed RGB 16/32, CMYK, Gray (8/16/32), Lab and Duotone. Indexed and Bitmap refuse the
  add by name, and a conversion into either refuses while a spot channel exists; every other mode and depth
  conversion carries the ink, `convert_precision`'s flattening steps included.
- **The overprint** (`spot_overprint`, Doctored's own rule, unmeasured): each spot channel in list order, the ink
  source-over the composite at weight coverage / max, blended `B = (1 − s)·Multiply(Cb, ink) + s·ink` on encoded
  values; coverage 0 leaves a pixel exactly as it was. The ink is encoded through `ColorPipeline` from its Lab
  twin into the composite's profile (relative, no BPC). Typed (`spot_typed`): RGB 16/32, Duotone on its RGB view,
  Gray by the ink's L*, Lab through ProPhoto with coverage-0 pixels' codes kept. A 32-bit composite (RGB or Gray)
  is linear light, so the rule runs on its display encoding (`display_encoding`: the profile's curve, a logarithm
  above white, no tone map or exposure) and is decoded back unclamped, as a display-referred filter runs: its
  flattened exports and Merge Spot Channel keep what the view shows, and a value above white survives. CMYK
  overprints in INK on paper (`spot_cmyk`: the ink's CMYK through the profile's B2A; no B2A refuses by name).
- **The composite view** overprints each visible spot channel; `render` (full, region, preview_divisor) the same.
  A 16- or 32-bit document's view lays each ink on its 8-bit display (`spot_view`, once per channel); a float
  window's at its own exposure and candidate tone map, and HDR Preview stands down while a spot overprint shows,
  since that is drawn on the SDR view over the view slot.
  **Merge Spot Channel** flattens first (Flatten Image's rules), bakes the one ink and deletes it, one undo step;
  Duotone refuses. **Alpha ↔ spot** inverts the plane both ways (`max − v`), one undo step each.
- **`.doctored`:** conditional word 112 and its `SPOT` table after `LREF`, written only when a channel is spot;
  `DRDC_VERSION` unchanged.
- **Which files carry them** is ONE rule, `psd_export_preflight::spots_written_for`, asked with the document's real
  depth by every preflight, report and save; the host's mirror of its flat-TIFF case is
  `SpotChannels.flatTiffKeeps`, so a flat TIFF save or copy asks the core to keep them only where it writes them.
  It holds for a PSD or PSB of RGB, CMYK, Gray or Duotone at 8 or 16 bits (a 16-bit Duotone document's file is
  written at 8 bits); a TIFF, flat (`SpotFile::FlatTiff`) or layered (`SpotFile::Layered(Tiff)`, 1c-2b), of RGB or
  CMYK at 8 or 16 bits. A Lab or 32-bit PSD/PSB refuses with `PSD_REFUSAL`; a Gray, Duotone, Lab or 32-bit layered
  TIFF and a Lab TIFF save with `TIFF_REFUSAL` (`preflight::spot_refusal` picks the sentence by container); PDF in
  the host (`pdfRefusal`, the measured reason). A flat Grayscale or 32-bit TIFF save or copy composites the
  overprint and names the loss, as every other flattened save or export does (`flat_save_losses`' line, the
  `spot_channels` reply key). Alpha (non-spot) channels stay a disclosed loss in PSD, PSB and TIFF.
- **PSD/PSB** (1b-1): a saved channel is spot when its 1077 DisplayInfo kind is 2, its ink colour space 7 (Lab ×
  100, a and b signed), its opacity the Solidity; the plane is stored `max − coverage` (`psd_spot_records::stored`,
  the one map). Resources 1006/1045/1077/1053 (ids from 3), every list led by "Transparency" with a negative
  layer count. The writer writes the first `56 − composite planes` spot channels and names the rest; a ZIP
  composite's spot planes, an unreadable plane, a resource mismatch, a non-Lab ink and an ink past `SpotInk::new`
  are loss lines, never a refused open.
- **Flat TIFF** (1c-2a): a spot channel is a TRAILING extra sample after the colour samples and any transparency,
  ExtraSamples 0, chunky, stored `max − coverage` in RGB and CMYK alike (`tiff_spot_samples::tiff_stored`); names
  and inks are the PSD's four resources in tag 34377, led by "Transparency" when the file has one. The reader
  keeps every lossless compression (`tiff_directory::Overlay`, one Multiband decode) by the PSD reader's rules
  (`spot_candidates`); planar samples, a rotated Orientation, an unread compression and image data the file ends
  inside of (`saved_chunks_held`, from the directory alone: an uncompressed strip or tile short of the samples the
  decoder reads, a compressed one starting at or past the end) are a loss line in the records the import report
  and every open read. A compressed strip that starts inside the file but does not decode (cut short or
  corrupt) passes the records: a flat file's main decode fails on it first, and the layered import report decodes
  a compressed composite itself a strip at a time (`saved_planes_decode`, the open's decoder), so it names the kept planes lost,
  which is the byte open's one place for that line; the typed layered open adds none. The writer
  lays uncompressed strips, a transparency only when some pixel is not opaque (ExtraSamples 2), then the
  spots, their resources after the filtered inherited run (`DROPPED_8BIM`).
- **Layered TIFF** (1c-2b): Photoshop's own layered TIFFs (take2's `rgb8_l.tif`, `cmyk16_l.tif`) follow the flat
  rule, so the layered TIFF reads and writes spot channels under it. Write (`tiff_layers_write::write_spots`):
  the composite strip's colour and alpha samples, then each written spot's `SpotCoverage::stored` big-endian,
  ExtraSamples `[2, 0 × n]`, the resources (`encode_resources`, the lead always, a layered TIFF carrying its
  alpha) appended to the image resources in tag 34377, beside tag 37724; the planes are borrowed in place, no
  new plan term (the preflight's `spot_bytes`, now counted for TIFF). A CMYK flattened TIFF export is admitted
  at the flat CMYK TIFF save's own plan (`cmyk_save_scratch_of(SaveFormat::Tiff)`). Read: a TIFF container's
  `spot_records` is `tiff_spot::layered_spot_records` on its own file and resources, so the import report names
  them, and both layered opens (`with_layered_tiff_spots`) run the flat open's one body; a typed layered open
  adds only the lines it finds itself (the channel limit, an unreadable plane) to its warnings. A spotless TIFF
  writes and reads exactly the bytes it did before.
- **PDF stays out.** Doctored's PDF cannot write spot channels as `/Separation` colourants (the form 1c-2a's
  take2 PDFs both measured Photoshop's PDF keeping them in), and no core PDF reader exists: Doctored's PDF open
  rasterizes the page through `CGPDFDocument`, so no spot channel is read. `pdfRefusal` names that reason.
- **Memory.** Every plan term the spot paths add (the view, the typed overprint, merge, kind change, the flat
  TIFF's five terms) is held by `scripts/spot-admission/measure.py` (Release, 24 MP, 3 runs, every case PASS).

## Photoshop's measured facts

Photoshop 26.10.0 throughout.

- **PSD/PSB authored** (`spot_psd_take.py` take1, seven files): kind 2, Lab space 7, H1 `max − coverage` on all
  seven, the "Transparency" lead with merged alpha; Photoshop's 1006 is Mac Roman (Doctored writes UTF-8; both
  readers prefer 1045).
- **PSD/PSB read back** (`spot_psd_readback_take.py` take1 and take2, run `20261025T152228Z-63b5d535`): Photoshop
  opens Doctored's RGB, CMYK and Gray at 8 and 16 bits and Duotone at 8, PSD and PSB, with every spot's name,
  order, Lab, Solidity and coverage as written; it opens no 16-bit Duotone PSD/PSB, with or without spots, hence
  the 8-bit write.
- **TIFF and PDF authored** (`spot_tiff_take.py` take2, run `20261026T230703Z-a8b8762a`): PLACE (trailing extra
  samples, ExtraSamples 0; Photoshop's transparency 1), L1, H1 in RGB and CMYK, C0 (no overprint in the
  composite); the layered files the same layout beside tag 37724; PDF-FORM SEPARATIONS.
- **TIFF read back** (`spot_tiff_readback_take.py` take1, run `20261027T010409Z-ce273362`): Photoshop opens
  Doctored's flat and layered TIFFs in RGB and CMYK at 8 and 16 bits (nine files, one flat with a transparency,
  the non-ASCII name) in their mode and depth with every spot's name, order, kind, Lab and Solidity exact and
  coverage within the bounds (the block exact, the ramp within 1 / 4 codes), each layered file with its two
  layers. Its copies' 1077 records equal Doctored's byte for byte; the planes equal at 8 bits, the 16-bit ramp
  within 1 code.

## Shipped

- **1a-1** (`051d3cbe`): `SpotInk`, the add and set, the overprint kernel, `.doctored` word 112 and `SPOT`, the
  Indexed/Bitmap refusals.
- **1a-2** (`18b81bc6`): FFI, the Channels menu and sheet, the composite view's overprint, MCP
  `add_spot_channel` / `set_spot_channel`; drive S1–S8.
- **1b-1** (`7e83e17a`): PSD/PSB in and out in Photoshop's layout, the fixtures.
- **`convert_precision` keeps the channels** (`fcf5ce2e`): drive S12.
- **1c-1** (`7ea3df81`): Merge Spot Channel, alpha ↔ spot, the typed exports' and the region render's overprint;
  drive S9–S11.
- **1b-2** (`71afa269`): the PSD/PSB readback take; a 16-bit Duotone PSD/PSB written at 8 bits.
- **1c-2a** (`12b8a5a5`): flat TIFF in and out, the author take and fixtures, PDF measured and kept refused; drive
  S13.
- **1c-2b** (2026-10-26, the closing batch): the layered TIFF in and out (`write_spots`, `SpotCoverage`,
  `layered_spot_records`, `with_layered_tiff_spots`, `spot_refusal`), the TIFF readback take PASS; tests
  `spot_tiff_layers.rs` and the layered fixtures' test in `spot_tiff_photoshop.rs`; drive S14.
- **32-bit verified** (2026-11-12): the float arm runs on the display encoding (it had laid an encoded ink on
  linear samples); a float window's overprint follows its exposure and candidate map and stands HDR Preview down;
  `get_view`'s image is the drawn channel base; Mode ▸ Grayscale from 32-bit RGB builds the twin's float policy (it
  kept the source's curve). Tests `spot_float.rs` and `gray_export.rs`'s converted 32-bit case; drive S15–S16;
  probe "float spot view", "hdr spot view".

## Open items

- The overprint rule is Doctored's own, not measured against Photoshop
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- Merge Spot Channel and the alpha ↔ spot kind change are declared, not measured against Photoshop
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
- A 16-bit Duotone PSD/PSB export's 8-bit copy is admitted at the depth step's own plan (1.68 GB planned against
  65 MB measured at 24 MP), over-admission kept by the controller's ruling
  ([#1](https://github.com/kpgalligan/doctored/issues/1)).
