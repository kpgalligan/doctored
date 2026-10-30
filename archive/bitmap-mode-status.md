# Bitmap document mode (#1): status

- **Status:** Open — phases 1a (`4d226f92`), 1b (`aaf2593e`), 1c (`f7f07122`), 2a-i (`baf92c05`), 2a-ii
  (`25eed3cd`), 2b (`e2dd2552`) and 3 (this batch: what the walk left, built through the adapter or refused
  for good; no refusal names a phase) shipped; the close (below) is the next batch.
  The decisions in force and the shipped phases' lines are in [bitmap-mode-shipped.md](bitmap-mode-shipped.md)
- **Issue:** [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella; Grayscale is
  `designs/grayscale-mode.md`, Indexed Color `designs/indexed-mode.md`, Multichannel
  `designs/multichannel-mode.md`)
- **Code:** `core/src/bitmap_document.rs`, `bitmap_method.rs` (kernel), `bitmap_convert.rs`, `bitmap_native.rs`,
  `bitmap_codec.rs` (kernel), `bitmap_io.rs`, `bitmap_export.rs`, `bitmap_psd.rs`, `bitmap_edit.rs`,
  `bitmap_tools.rs`, `ffi_bitmap.rs`; `app/Sources/BitmapDocument.swift`, `RasterCore+Bitmap.swift`,
  `BitmapModeSheetController.swift`, `EditorViewController+BitmapMode.swift`, `AgentCatalog+Bitmap.swift`,
  `EditorViewController+CmykMode.swift`'s bitmap arms; the take `scripts/photoshop-probe/bitmap_take.py`
- **Template:** Indexed Color phase 1a (`451c329f`): the same seams, gate shape and phase numbering; Grayscale
  is the source mode

## Phases

Phase 3 shipped (its lines and decisions 7, 11 and 14 are in the companion). What remains is the close.

### Close (contract, the next batch)

- Condense this doc and [bitmap-mode-shipped.md](bitmap-mode-shipped.md) into the ADR
  `designs/bitmap-mode.md`: the decisions in force, the known differences below, the measured section's
  verdicts and the admission headline rows, each with its command.
- `git mv` both originals to `archive/` unedited (into `archive/<YYYY-MM>/` when `archive/` already holds the
  basename) and add their two lines to `archive/INDEX.md`.
- Update README's Bitmap paragraph (its "The rest refuse by name" sentence and the tool list) to the shipped
  mode, and repoint any doc link to the status doc at the ADR.
- Post the #1 comment for the Bitmap close.

## Known differences (for the close's ADR)

- **Lens Blur** (RULING-1): walk2 found Filter ▸ Blur ▸ Lens Blur the one Filter leaf enabled in Bitmap
  mode. No phase builds it: `apply_filter` refuses for good on a Bitmap document (the core's `filter` op in
  `DISABLED_OPS`), and `filter` `lens_blur` refuses by its own sentence, "Lens Blur on a Bitmap
  document is not built in Doctored; convert to Grayscale first."
  (`BitmapDocument.lensBlurRefusal`).
- **Halftone Screen** (RULING-1): Rasterize's declared spot rule (AREA-CCW-CORNER, decision 4), unmeasured
  for good in this phase: two Photoshop sessions (1b's smoke1, 2b's halftone1) hung on its conversion
  (−1712) with different descriptors, and what else the HlfS descriptor needs cannot be read without a
  ScriptListener. `bitmap_photoshop_take`'s halftone test SKIPs with that reason.
- **Diffusion Dither**: Sierra Lite serpentine, disclosed as not matching (decision 4); Photoshop's own
  diffusion varies between sessions, so there is no further fit.
- **Save Selection** (phase 3): walk2 measured Select ▸ Save Selection enabled in Bitmap mode, but a Bitmap
  document holds no alpha channel (decision 1, `.rz` unchanged), so it is not built: "Save Selection on a
  Bitmap document is not built in Doctored: a Bitmap document holds no alpha
  channel; convert to Grayscale first." (`BitmapDocument.saveSelectionRefusal`). The alpha-channel tools
  and Load Selection refuse by `channelRefusal`.
- **Liquify's workspace preview** (phase 3): the Liquify workspace previews the gray proxy (the Gray
  storage's warp, unthresholded); Apply thresholds it onto the plane.

## The leaf rule (RULING-1)

The walk's row rule stands for rows whose leaves map to no single tool. Where a measured leaf maps one to
one to a tool, that tool is decided on its own, not by its row. Arbitrary rotation refuses for good (walk2
measured Image ▸ Image Rotation ▸ Arbitrary DISABLED): `rotate` at an angle that resamples refuses ("Arbitrary
rotation is not available in Bitmap mode.", `BitmapDocument.arbitraryRotationRefusal`),
Image ▸ Rotate Arbitrary… is disabled, and the core's `rotate_canvas` joins `DISABLED_OPS`
(`geometry_rotate_canvas` left `BITMAP_FAMILIES`); the quarter turns and flips stay. By the same rule
`depth_blur` (Filter ▸ Neural Filters ▸ Depth Blur, outside the walk's rows, whose Neural Filters leaf was
measured disabled) refuses for good. No other disabled leaf maps to an admitted tool alone: the rest sit
in DISABLED rows, in rows whose tools were already refused for good, or behind `apply_filter`. Free
Transform, Move and Image Size follow their own leaves as measured; the Crop tool's straighten follows
Image ▸ Crop (enabled).

## Photoshop in Bitmap (measured)

Photoshop 26.10.0, `bitmap_take.py` (receipts `~/devel/rasterize-fixtures/photoshop/bitmap-mode-547/`, never
committed; every gate declared in the docstring before its session; each session under the probe lock, with
the pasteboard restored and Photoshop quit):

- **smoke1** (1b): the −1712. Its `photoshop.json`, written after the dialog was dismissed, records HR and HL
  as "User cancelled" and `#Lns` resolving to `""`: no Photoshop unit, so the HlfS object's `Frqn` was
  unreadable and CnvM opened the Halftone Screen dialog under DialogModes.NO. Fixed: `Frqn` in `#Rsl` and an ID
  guard (an unregistered ID records `unregistered id <code>` and never calls CnvM).
- **fit1** (offline, smoke1's TIFFs): Pattern consistent with an 8 × 8 threshold table at all 64 positions
  (interval widths ≤ 7; PT8-MID, LO and HI 16384 / 16384, Bayer 8 8876); Diffusion: the best of 64 kernels,
  LITE-SERP-FREE-128, 9982 / 16384.
- **take1** (scene2, 256 × 64): T CONFIRMED (TH128 16384 / 16384); P PASS, PT8-MID 16253 / 16384 (99.20 %;
  LO 16183, HI 16143, Bayer 8 10655); D MISS-FITTED, LITE-SERP-FREE-128 11460 on D and D2 (69.9 %) against
  Floyd–Steinberg's 11250, D == D2 within the session but smoke2's D different (11378 of 16384 equal):
  Photoshop's diffusion varies between sessions and is disclosed as not matched.
- **walk2** (the walk's record; walk1 equal but for its unread Bitmap counts, Bitmap having no `histogram`
  property): the menus VALID (495 items each). DISABLED in Bitmap: Edit ▸ Free Transform and Transform,
  Perspective Warp, Assign and Convert to Profile, Image ▸ Trim, Apply Image and Calculations, Layer ▸ New
  Layer, Layer via Copy and Cut, Duplicate Layer, New Fill Layer, Select ▸ Color Range, Subject, Sky, Select
  and Mask, Similar and Grow, Neural Filters. ENABLED: Cut/Copy/Clear, Fill, Stroke, Image Size, Canvas Size,
  Image Rotation (Arbitrary excepted), Crop, Select ▸ Modify, Transform Selection, Quick Mask, Save Selection,
  Liquify, Lens Blur. Fill's blend modes on Bitmap: Normal, Darken, Lighten and Dissolve accepted, the other 23
  refused; a black fill over the selection (40, 8, 120 × 40) of the thresholded scene2 left 10745 black pixels
  in Normal and Darken and 8330 in Lighten, each the W3C prediction (Dissolve 10745, no prediction). The wand
  CONFIRMED refused ("does not work with Bitmap images").
- **halftone1** (last, its own session): STOPPED. With `Frqn` in `#Rsl` and every ID registered, the HR
  conversion still never returned (−1712) and Photoshop stayed running after the quit (pid 95305, most
  likely on the Halftone Screen dialog again), so it was left for the owner and halftone2 was not run.
  Halftone Screen stays declared (AREA-CCW-CORNER) and unmeasured for good in this phase (RULING-1).

## Admission (headline rows)

`RZ_VERIFY_APP=build/Build/Products/Release/Rasterize.app python3 -B scripts/bitmap-admission/measure.py
--runs 3` (24 MP; plan ≥ ⌈1.25 × max E⌉; 1b's run for the conversions, 1c's (`--cases
open-bitmap-png,open-bitmap-tiff,open-bitmap-psd,export-bitmap-tiff,export-bitmap-psd`) for the files, 2a-i's
(`--cases brush-bitmap,rotate90-bitmap,crop-extend-bitmap`) for the edits at 34 B/px, and 2a-ii's (`--cases
rotate5-bitmap`, then `--cases transform-bitmap`) for the resampled geometry, unchanged at 34; 2b's for
`to-bitmap-diffusion`, whose scratch is now two error rows; 2b retired the two 2a-ii cases with their tools):

| Case | Plan | max E | Ratio |
|---|---:|---:|---:|
| `to-bitmap-threshold` | 985,048,576 | 771,785,256 | 1.276 |
| `to-bitmap-diffusion` (2b's Sierra Lite, `--cases to-bitmap-diffusion`; 1.320 with 1b's memo) | 985,144,576 | 770,605,632 | 1.278 |
| `to-bitmap-resample` (1.25 × ppi; 2.108 before 1b's fix) | 1,572,463,008 | 1,208,960,056 | 1.301 |
| `to-bitmap-halftone` (53 lpi, 45°, round) | 985,572,864 | 771,260,968 | 1.278 |
| `to-bitmap-custom` (inline 512 × 512 tile) | 985,310,720 | 778,338,904 | 1.266 |
| `to-gray-from-bitmap` | 985,048,576 | 576,438,680 | 1.709 |
| `to-gray-ratio-4` | 62,548,576 | 38,567,960 | 1.622 |
| `open-bitmap-png` (1c) | 1,009,071,434 | 771,916,328 | 1.307 |
| `open-bitmap-tiff` (1c) | 1,012,048,710 | 771,686,928 | 1.311 |
| `open-bitmap-psd` (1c, RLE) | 1,012,080,616 | 771,473,936 | 1.312 |
| `export-bitmap-tiff` (1c, kept) | 49,048,576 | 6,209,536 | 7.899 |
| `export-bitmap-psd` (1c, flattened) | 7,080,644 | 3,489,792 | 2.029 |
| `brush-bitmap` (2a-i, size 200) | 1,837,516,356 | 595,853,768 | 3.084 |
| `rotate90-bitmap` (2a-i) | 1,397,269,816 | 772,538,920 | 1.809 |
| `crop-extend-bitmap` (2a-i, 7000 × 5000, Delete Cropped off; 1.168 at 31 B/px) | 1,196,294,406 | 934,445,728 | 1.280 |
| `rotate5-bitmap` (2a-ii, 5° bicubic; the case and arbitrary rotation gone since 2b) | 1,786,281,238 | 918,733,472 | 1.944 |
| `transform-bitmap` (2a-ii; the case and the tool gone since 2b) | 1,632,682,566 | 772,883,008 | 2.112 |

## Open items

- None beyond the close. Halftone Screen's measurement is closed by RULING-1 (known differences, above).

## Shipped runs

Moved to [bitmap-mode-shipped.md](bitmap-mode-shipped.md): the decisions in force and each shipped phase's lines.
