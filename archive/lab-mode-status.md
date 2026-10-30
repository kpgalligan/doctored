# Lab Color document mode (#547): status

- **Status:** Open — phase 1 (the plan, the model, New, Image ▸ Mode both ways, the view, `.rz`) shipped in
  `d4cf7e32`; phase 1b (layer structure, Fill, alpha channels, the Lab TIFF, exports through RGB, the fit-first
  thumbnail) in `fc013245`; phase 1c (the panels and readouts) in `740003b7`; phase 2's Photoshop take (the
  measured blend gate) in `e6c31f84`; phases 2a (painting) and 2b (adjustments) in `d009826a`; phase 2c
  (filters, Fade, the colour adjustments, Auto, Equalize) in `38f5a091`; phase 2d (the colour ops through
  ProPhoto RGB, plane statistics, mask/alpha filters, Color Samplers, RGB → Lab carrying adjustment layers) in
  `ed2f675d`; phase 2e (retouch and content-aware tools, layer effects, the Lab eyedropper ink, the Lightness,
  a, b and alpha targets, the Mode popups) in `fb60e994`; phase 2f's first part (the five luma filters on the
  layer in one-channel passes, Offset's fill, the channel Levels/Curves and Blend If carries, Blend If on
  Lightness, a and b, Levels Auto) in `be2d0252`; 2f's second part (Pattern Overlay and Pattern strokes,
  Stroke…, the Fill and Gradient Mode popups) in `11184e3d`; 2f's third part (Remove, the Art History Brush,
  Red Eye; the default phase becomes 6) in `71c6b5d6`; phase 3's compositor (the Lab arms of every
  mode, take2 of `lab_blend_take.py`; Darker Color, Lighter Color and the four HSL modes join) in
  `c47305cb`; phase 3a (opening Lab TIFF, PSD, PSB and layered TIFF files, take2 of `lab_files_take.py`) in
  `4b67b0a3`; phase 3b (Export as PSD writing Lab PSD, PSB and layered TIFF, take1 of
  `lab_export_take.py`, and the Lab TIFF open's own charge) in `1e20fde8`; phase 4a (the resamples and
  transforms on the codes) in `06acd034`; phase 4a-ii (the warps, Liquify, Align and Distribute, the Move
  tool) in `da5a9431`; phase 4b-i (the selected pixels, Layer Via, Stamp Selected, Save and Load Selection,
  Select ▸ Modify) in `aa55803b`; phase 5a (Lab → CMYK directly, 8/16 Bits/Channel) in `e88d8164`; phase 4b-ii
  (sampled Color Range, take1 of `lab_select_take.py`; the Magic Wand and Select Similar did not ship) in
  `9200efcd`; phase 5a-ii (Lab → Grayscale directly, take1 of `lab_gray_take.py`) in `7be03800`; phase 4c (the
  clipboard across modes, Duplicate to New Document; it closes phase 4) in `8e7fa3ff`; phase 5b (proof, Convert
  to Profile, Assign refused; take1 of `lab_cmyk_take.py`) in `5f2f3892`, so phases 4 and 5 are complete;
  phase 6a-i (type, shape and fill layers, paths and vector masks) in `e0d670c5` and 6b-i (Histogram, Apply
  Image, Calculations, Stack Modes, Merge to HDR, every Color Range range, the planes' selected pixels) in
  `bf95b803`; phase 6a-ii (comps, links, snapshots, the legacy ops, Clear, Convert to Profile's Gray
  destinations, a plane's vacated value) in `9aa9a847`; phase 6b-ii (Auto-Align, Auto-Blend, Merge Panorama,
  Quick Mask, Quick Selection, the model selections, Select and Mask) in `50de4191`. Phase 6b-iii (the neural
  and generative tools on the Display P3 view; Sky Replacement refused for good) is this batch's; the close is
  next
- **Issue:** [#547](https://github.com/kpgalligan/doctored-dev/issues/547) (the colour-mode umbrella; CMYK is
  `designs/cmyk-mode-status.md`, Grayscale `designs/grayscale-mode.md`)
- **Code:** `core/src/lab_document.rs`, `lab_convert.rs`, `lab_native.rs`, `lab_rgb.rs`, `lab_save.rs`,
  `lab_encode.rs`, `lab_adjust.rs`, `lab_tone.rs`, `lab_filter.rs`, `lab_channel_filter.rs`,
  `lab_retouch.rs`, `lab_style.rs`, `lab_blend.rs`, `lab_select.rs`, `lab_stack.rs`, `lab_tiff_read.rs`,
  `psd_import_lab.rs`, `psd_export_lab.rs`, `lab_limits.rs`, `lab_cmyk.rs`, `lab_depth.rs`, `lab_gray.rs`,
  `lab_clipboard.rs`, `lab_artwork.rs`, `lab_comps.rs`, `lab_neural.rs`, `ffi_lab.rs`, `icc_builtin.rs`
  (`lab_twin_blob`);
  `app/Sources/LabDocument.swift`, `LabAnalysis.swift`, `LabAdjust.swift`, `LabInk.swift`,
  `LabDepthEdit.swift`, `LabClipboard.swift`, `LabProof.swift`, `EditorViewController+LabProfile.swift`,
  `AgentServer+LabColour.swift`, `RasterCore+Lab.swift`, `AgentCatalog+Lab.swift`,
  `EditorViewController+LabMode.swift`, `EditorViewController+CmykMode.swift`'s lab arms
- **Template:** Grayscale document mode, phase 1a (`fa9c7b5b`): the same seams, gate shape and phase numbering

## Phases

The decisions in force and the shipped phases (1 to 2f, 3's compositor, 3a, 3b, 4, 5, 6a-i, 6a-ii, 6b-i,
6b-ii and 6b-iii) are in
[lab-mode-shipped.md](lab-mode-shipped.md).
The items measured disabled (below) refuse for good.

### 3a and 3b — opening and writing Lab files

Shipped; their few lines, the measured encodings and the written layout are in
[lab-mode-shipped.md](lab-mode-shipped.md).

### 4 — geometry, selections, clipboard (closed)

Shipped: 4a (`06acd034`), 4a-ii (`da5a9431`), 4b-i (`aa55803b`), 4b-ii (`9200efcd`) and 4c (`8e7fa3ff`); their
lines are in [lab-mode-shipped.md](lab-mode-shipped.md). Phase 4 holds nothing more: `LATER_PHASES` and
`laterPhases` no longer list "4". The Magic Wand (and the Paint Bucket's Lab rule) and Select Similar and Grow
refuse naming their takes' issues (Open items).

### 5 — colour (complete)

**5a shipped** (`e88d8164`), **5a-ii shipped** (`7be03800`) and **5b shipped** (this batch); phase 5 is
complete and its lines are in [lab-mode-shipped.md](lab-mode-shipped.md): Image ▸ Mode ▸ CMYK Color from Lab
through 16-bit ProPhoto RGB (`lab_cmyk`), 8/16 Bits/Channel (`lab_depth`), Image ▸ Mode ▸ Grayscale from Lab
on L\* alone (`lab_gray`, confirmed by `lab-gray-547/take1`), then the proof of the Display P3 view and
Convert to Profile as a mode conversion, with Assign Profile refused (no document profile, 5b). **Lab → CMYK is
NOT confirmed by Photoshop:** `lab-cmyk-547/take1` read MISS at the 1-code gate, max/mean 6.06/1.04 codes at
8 bits and 5.88/1.02 at 16, C1 (today's route) = C2 (Lab PCS to the profile's B2A), so the residual is most
likely in the → CMYK conversion itself; 5a's route stays shipped against its own oracle, and the follow-up is
one RGB → CMYK take with intent, BPC and dither candidates.

### 6 — the rest of the RGB surface (contract)

The default phase: since phases 4 and 5 closed, `lab_document::LATER_PHASES` / `LabDocument.laterPhases` are
empty, so every refused tool `AWAITING_TAKE` does not name refuses naming 6. Each lifts through the existing kernels on the L\*a\*b\*
codes (per channel, or through ProPhoto RGB or the Display P3 view where it is a colour operation) or refuses
for good by name where it is measured disabled in Lab. Proposed batches (Gray's phase 6 order):

- **6a-i shipped** (`e0d670c5`) and **6a-ii shipped** (`9aa9a847`; their lines are in
  [lab-mode-shipped.md](lab-mode-shipped.md)): type, shape and fill layers, Rasterize Artwork, paths and vector
  masks; then layer comps (`lab_comps`), links, snapshots, the legacy `erase`/`clone`/`heal` ops, Edit ▸ Clear,
  Convert to Profile's Gray destinations and the planes' vacated value (the background's code in that plane).
- **6b-i shipped:** Histogram, Apply Image, Calculations, Stack Modes, Reduce Noise from Stack,
  Load Files into Stack, Merge to HDR, every Color Range range and the selected pixels of the Lightness, a, b
  and alpha planes; its lines are in [lab-mode-shipped.md](lab-mode-shipped.md).
- **6b-ii shipped** (`50de4191`; its lines are in [lab-mode-shipped.md](lab-mode-shipped.md)): Auto-Align
  with Vignette Removal and Geometric Distortion, Auto-Blend (Stack Images, Panorama with Seamless Tones),
  Merge Panorama, Quick Mask, Quick Selection, Select Subject and its family, Sky, Person Parts, Object Finder and
  Select and Mask with Decontaminate Colors.
- **6b-iii shipped** (this batch; its lines are in [lab-mode-shipped.md](lab-mode-shipped.md)): Super Zoom,
  JPEG Artifacts Removal, Neural Colorize and Generative Fill, Expand and Variations on the Display P3 view,
  results by the Mode rule (`lab_neural`); Sky Replacement refused for good.
- **The close:** every remaining tool given one fate (shipped, or refused for good by name) — among them
  `select_magnetic`, `add_luminosity_masks`, `distort_layer`, `import_channel` and the legacy `rasterize`
  PaintEdit op (no Lab caller), RGB/Gray → Lab carrying a Generative Layer (`first_lab_blocker` still names
  it) and Lab mode or Bits/Channel conversions keeping variations live — and the ADR.

## Photoshop in Lab (measured)

Photoshop 26.10.0, phase 2's take (`lab-mode-547/take1`); phases 2b's to 2f's contract for adjustments
and filters:

- No 32-bit Lab: Image ▸ Mode ▸ 32 Bits/Channel is disabled in Lab.
- Blend modes not offered at 8 or 16 bits: Darken, Color Burn, Lighten, Color Dodge, Difference, Exclusion,
  Subtract, Divide (`LAB_NOT_OFFERED`). Offered: the other 19.
- Disabled in Lab at 8 and 16 bits: Image ▸ Adjustments ▸ Exposure, Vibrance, Black & White, Channel Mixer,
  Selective Color, HDR Toning, Match Color; Image ▸ Auto Color; Filter ▸ Camera Raw Filter, Lens Correction,
  Vanishing Point, Render ▸ Difference Clouds, Render ▸ Lens Flare, Stylize ▸ Oil Paint, Stylize ▸ Solarize,
  Video ▸ NTSC Colors, Other ▸ HSB/HSL. At 8 bits also Image ▸ Mode ▸ Indexed Color, Filter ▸ Filter Gallery
  and Stylize ▸ Extrude (each disabled in 16-bit RGB too). Every other Image and Filter item reads as on the RGB
  walk (Hue/Saturation, Curves and Levels enabled).
- Of the documented list: Match Color and Lens Flare are disabled; the Filter Gallery is disabled at 8 bits
  (and in 16-bit RGB); Lighting Effects is not in the menu walk (NOT FOUND).
- 5b's take (`lab-cmyk-547/take1`, 8-bit Lab, Edit and View walks): Edit ▸ Assign Profile… disabled; Edit ▸
  Convert to Profile…, View ▸ Proof Setup, Proof Colors and Gamut Warning enabled. (a) Image ▸ Mode ▸ CMYK
  Color from Lab at 8 and 16 bits into "U.S. Web Coated (SWOP) v2": MISS, nothing ships.

## Open items

- The Lab Magic Wand (and the Paint Bucket's Lab rule): take1 fitted no declared candidate at 16 bits (the
  halved cyclic projections were exact at 8); it needs a separate Photoshop take.
- Lab Select Similar and Grow refuse through scripting though the menu shows them enabled; they need another
  invocation before a take ([#683](https://github.com/kpgalligan/doctored-dev/issues/683)). The Magic Wand's and
  Similar/Grow's refusals are `AWAITING_TAKE`'s; Similar/Grow's name #683.

- Lab → CMYK is not confirmed by Photoshop: take1's residual (6.06 codes max at 8 bits) is common to both
  candidates and most likely in the → CMYK conversion itself; the follow-up is one RGB → CMYK take.
  `lab_cmyk_photoshop.rs` records it.

## Shipped runs

Phases 1 to 5, 6a-i, 6a-ii, 6b-i, 6b-ii and 6b-iii and the decisions in force:
[lab-mode-shipped.md](lab-mode-shipped.md).
