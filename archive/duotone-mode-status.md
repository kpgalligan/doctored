# Duotone document mode (#547): status

- **Status:** Open — phases 1a (`00c31aa1`), 1b (`7f05bfe9`), 2a (`761abdd0`) and 2b (this batch: the take
  of the view, the Photoshop-written PSD readback, the menu walk) shipped; the close next
- **Issue:** [#547](https://github.com/kpgalligan/doctored-dev/issues/547) (the colour-mode umbrella; CMYK is
  `designs/cmyk-mode-status.md`, Grayscale `designs/grayscale-mode.md`, Lab `designs/lab-mode.md`, Indexed
  `designs/indexed-mode.md`)
- **Code:** `core/src/duotone_ink.rs` and `duotone_ado.rs` (kernels), `duotone_document.rs`, `duotone_native.rs`,
  `duotone_files.rs`, `duotone_convert.rs`, `ffi_duotone.rs`, `gray_document.rs`'s `GrayPolicy.duotone`;
  `app/Sources/DuotoneDocument.swift`, `DuotoneOptions.swift`, `RasterCore+Duotone.swift`,
  `DuotoneSheetController.swift`, `DuotoneInkSheets.swift`, `DuotonePreview.swift`,
  `EditorViewController+DuotoneMode.swift`, `AgentServer+Duotone.swift`, `AgentCatalog+Duotone.swift`
- **Template:** the Grayscale ADR (`designs/grayscale-mode.md`) and its seams, and Indexed phase 1a (`451c329f`):
  the same gate shape, word slot and phase numbering

## Decisions in force

- **The model.** A Duotone document IS a Grayscale document (8 or 16 bits, layers and all, exactly as Gray holds
  them) whose `GrayPolicy` carries a description: 1–4 inks (Monotone, Duotone, Tritone, Quadtone), each a name
  (1–64 characters), a D50 CIE Lab colour and a curve of 13 slots at the inputs 0, 5, 10, 20, 30, 40, 50, 60,
  70, 80, 90, 95 and 100 % (an output percent or blank; 0 % and 100 % set), plus one overprint colour per
  combination of two or more inks (authored Lab or the default). The descriptor keeps `color_mode` `"gray"` (the
  storage; the host's `isGray` stays true, so every Gray rule applies unchanged) and adds
  `"duotone":{"type","inks"}`; the host's `colorMode` answers `duotone`.
- **Ink colours are D50 Lab.** A PSD or `.ado` duotone specification can carry a Lab ink colour; Lab keeps the
  view and later conversions independent of the working RGB, and the Color Picker speaks D50 Lab.
- **The view (measured: phase 2b's take confirms it, below).** Per gray code g (the composite's R, u16),
  k = 1 − g/65535 (Gray's K % rule); each ink's coverage cᵢ = Sᵢ(255k)/255, Sᵢ Curves' natural cubic spline
  (`adjust_curves::Spline`, flat beyond its ends, clamped) through the set slots ×2.55; the coverage enters the mix
  through the document's Gray tone curve, c′ᵢ = 1 − kTRC(1 − cᵢ) (`GrayPolicy::decode_unit`), so Monotone Black on
  a straight curve reproduces the Grayscale display; the colour is the Neugebauer sum over every ink subset S of
  Πᵢ∈S c′ᵢ · Πⱼ∉S (1 − c′ⱼ) · P_S in PCS XYZ (D50): P_∅ the paper, Lab (100, 0, 0); P_{i} ink i's Lab; P_S for
  two or more inks the authored overprint, else W ⊙ Πᵢ∈S (Pᵢ ⊘ W) per component (the subtractive product
  W ⊙ Πᵢ (1 − c′ᵢ + c′ᵢ·Pᵢ/W) when nothing is authored). The XYZ goes through the twin's matrix (clamped to
  [0, 1]) and encoder into RGBA16 in the document's own profile, alpha kept, from a 65 536-entry table by gray
  code; then the ordinary preview. Overprint colours change the display only. The table is
  built once per use (`DuotoneViewTable`) and every image is mapped through it. Phase 2b's take
  (`duotone_take.py --take`, `duotone-mode-547/take1/decided.json`: VALID, decision A) measured it against
  Photoshop 26.10.0's Mode ▸ RGB Color of Rasterize-authored PSDs, over 26 patch centres in 8-bit RGB under the
  fixed gate (max |Δ| ≤ 3, every channel's mean ≤ 1): two inks [A] max 0.49, means 0.08 / 0.14 / 0.16; three
  inks [A] max 0.56, means 0.13 / 0.18 / 0.24; [B] (the sum on encoded sRGB) missed both (max 54.90 / 56.99).
  The mix ships unchanged.
- **The render** (`duotone_convert`) is the conversions' and exports' one use of the view: an RGB document in
  the twin, each raster leaf through the view, the record tree, masks, clipping, modes, a Gray Blend If, comps,
  channels, guides, paths, samplers, light, metadata and resolution carried. Blockers (first in postorder): an
  adjustment layer and a layer style with effects, named with `flatten_required`; `flatten` runs Flatten Image
  first. One view table a render, built on its source (after Flatten). Plan: raster pixels ×
  `RENDER_BYTES_PER_PIXEL` (8), the view's table, Flatten's scratch, bookkeeping.
- **Conversions out.** Mode ▸ RGB Color is the core's render then Convert to Profile (`convert_duotone_to_rgb`,
  Gray → RGB's body, its second stage admitted beside the source it still holds; its plan the render's plus
  Gray → RGB's), narrowed to 8 bits from an 8-bit source; CMYK
  Color, Lab Color and (8-bit) Indexed Color take the rendered document through their own routes; one undo step
  each; every target asks the Flatten question for a blocker. Mode ▸ Bitmap refuses (Grayscale first).
- **Exports** (Export, Quick Export, the preview, PDF, Export Layers, every format with profile document) render
  RGB through the view in the core (`export_stages` / `export_plan_for`'s Duotone arms over `without_gray()`, the
  plan its own plus the view's scratch), and every export reply carries `duotone` (`DuotoneDocument.exportNote`).
  A save that is not `.rz` refuses (`saveRefusal`): PSD and PSB are File ▸ Export as PSD's.
- **PSD and PSB** (colour mode 8, 8 or 16 bits). The colour-mode data is Photoshop's duotone specification
  (`duotone_ado`: 524 bytes, the layout read off the smokes' PSDs; Lab, RGB, HSB and Gray inks read, Lab written
  to 0.01 and curves to 0.1 %; an overprint equal at that precision to the default reads blank). Photoshop's file
  holds resources 1014 and 1017 (no 1018, no 1063). The open is the Gray open through `Container::duotone_gray`
  with the inks on the policy, which carry the file's block (the data and every 1014/1017/1018/1063 resource);
  the save is the Gray writer post-processed (`duotone_files::into_duotone_file`: mode 8, the carried data or
  `write_spec`, the carried resources), so unchanged inks write their data and resources back byte for byte, an
  ink change re-encodes. The block lives in memory only: `.rz` keeps the inks, not the bytes. A layered TIFF
  refuses. Measured in phase 2b: Photoshop opens Rasterize's PSD as Duotone with its inks and curves, and its
  own save holds colour-mode data byte-equal to Rasterize's (both cases), adds 1014 and 1017 and drops the ICC
  (1039: Photoshop's Duotone PSD carries no profile); `duotone3-curves.psd` is that save, read back by
  `duotone_files.rs`.
- **`.ado`.** A Duotone Options file is the same specification (`rz_duotone_read_ado`/`write_ado`; the sheet's
  Load…/Save…, `set_duotone path`, `get_duotone path overwrite`). No Pantone or licensed library ships.
- **Memory.** Mode ▸ Duotone… shares every sample: its plan is canvas × `TO_DUOTONE_BYTES_PER_PIXEL` (31) and
  bookkeeping; the display charges the view or the view beside its preview. Phase 1b's render, conversions and
  exports are held by `scripts/duotone-admission/measure.py`'s six new cases, phase 2a's profile edits (Gray's
  plan) by `assign-duotone16` and `convert-duotone16`; a miss raises the new term only.
- **`.rz`.** Word 109 replaces the Gray word 106 in the CMYK slot; `GRAY` then `DUOT` where the CMYK table goes
  (`duotone_native`'s module doc has the layout). Float storage under 109 refuses. `RZDC_VERSION` stays 89.
- **Phase 2a, corrected by phase 2b's menu walk** (Photoshop 26.10.0's `menuBarInfo` on the opened two-ink
  Duotone document and on its Grayscale conversion, a selection held in both; `take1/decided.json`'s `walk`,
  valid):
  - Assign and Convert to Profile: DISABLED in Duotone, so both refuse for good by tool and by selector
    (`disabledTools`: "<tool> is not available in Duotone."). 8 ↔ 16 keeps the inks.
  - Proof Setup and Proof Colors: DISABLED in Duotone (every Proof Setup entry and Proof Colors), so
    `get_proof_setup`, `set_proof_setup` and `render_proof` refuse for good and the View items are disabled
    (Proof Setup's presets and working plates too, and Gamut Warning, which would turn Proof Colors on).
  - New from Clipboard of a Duotone copy is a Grayscale document of the copied gray at the source's profile and
    depth (the payload drops the inks, as RZCB v5 holds it).
    Unmeasured for good (read only through File ▸ New, a dialog).
  - The Layers and Channels thumbnails go through the inks (`rz_precision_doc_duotone_thumbnail_rgba8`, fitted
    at u16 first); the Layers cache keys a Duotone document's entries on its inks and profile; unmeasured for
    good (no dialog-free reading). The Channels row is named after the type (Monotone, Duotone, Tritone,
    Quadtone; `get_duotone`'s `channel_name`): CONFIRMED (Photoshop names the channels Duotone and Tritone).
  - Duotone Options' Preview (on at open) shows the canvas through the sheet's inks; view-only, no MCP twin;
    unmeasured for good (a dialog).
- **What refuses.** The profile and proof tools and items, for good (above). Merge to HDR is ENABLED in
  Duotone, so it is admitted as Gray admits it (drive D23: the same answer as the Grayscale twin). 32
  Bits/Channel refuses for good (CONFIRMED disabled by the walk); Mode ▸ Bitmap asks for Grayscale first,
  though the walk found it enabled in Duotone (recorded for the close). Everything else a Gray
  document admits runs unchanged.

## Phases

### 1a — the plan, the model, the sheet, the view, Duotone ↔ Grayscale, `.rz`, MCP (`00c31aa1`)

Image ▸ Mode ▸ Duotone… on 8/16-bit Grayscale (and on Duotone, editing the inks), Grayscale back dropping them,
`.rz` word 109, MCP `convert_mode duotone`/`gray`, `get_duotone`, `set_duotone`; drive D1–D9.

### 1b — PSD/PSB, the conversions out, exports, `.ado`, the sheet fixes (`7f05bfe9`)

The decisions above (render, conversions, exports, PSD/PSB, `.ado`); the sheet's ink L*, a*, b* and Overprint
fields refuse text that is not a number (beep, focus, the sheet stays open). Core tests `duotone_files`,
`duotone_convert`, `duotone_photoshop_take` (SKIPs: no take); drive D10–D16; the smokes' two PSDs are
`core/tests/fixtures/psd_photoshop/duotone/`.

### 2a — profiles, proof, thumbnails, the Channels row, Preview, New from Clipboard (`761abdd0`)

The phase 2a items, built unmeasured (since corrected by 2b's walk). From
1b's review: the render builds one view table and admits Duotone → RGB's second stage beside the source. Core
tests `duotone_surfaces`; drive D17–D21; admission cases `assign-duotone16`, `convert-duotone16`.

### 2b — the take, the Photoshop-written PSD, the menu walk (this batch)

By the controller's ruling no further `curve` descriptor search: Rasterize authors the take's PSDs
(`core/examples/duotone_take_psd.rs`) and Photoshop opens, converts and saves them (`duotone_open` job).
`duotone_take.py --smoke 5`, `--smoke 6` and `--take` (receipts `duotone-mode-547/smoke5`, `smoke6`, `take1`,
each clean: Photoshop launched and quit, no stall or modal, the pasteboard restored). Smoke 5's two readback
fixes (g from Photoshop's own PSD composite, since its Duotone → Grayscale renders the inks; an empty menu
`name` takes its `title`) are in the runner's smoke log. The view's model is measured ([A] ships, numbers
above); the readback and the walk are in the decisions above. Core tests `duotone_photoshop_take` (the record
re-judged: gate, `SHIPPED`, the codec against `photoshop.inks`, `WALK_ROWS`) and `duotone_files`'
`photoshops_save_of_a_rasterize_duotone_reads_back` (`PHOTOSHOP_THREE`, `PHOTOSHOP_DIFFERS` empty); drive D17–D19
now refusal checks, D22 (the walk's `final` table), D23 (Merge to HDR as on Grayscale).

### What remains for the close

- Photoshop's Duotone → Grayscale renders the inks into the gray (take1's `gray.tif`: code 0 reads 17/18 with
  two inks, 14/15 with three; 255 reads 235/236), where Rasterize's Mode ▸ Grayscale drops the inks and keeps
  every code: a decision for the close.
- The walk's `bitmap` row found Image ▸ Mode ▸ Bitmap enabled in Duotone; Rasterize asks for
  Grayscale first. Duotone → Bitmap is not built.
- The walk's 25 unmapped paths (enabled in Gray, disabled in Duotone, named by no row; recorded only, no
  refusal changed): File ▸ Save and Revert (the Duotone document was unmodified at the walk); File ▸ Automate ▸
  Crop and Straighten Photos; Edit ▸ Content-Aware Fill, Content-Aware Scale, Puppet Warp, Perspective Warp;
  Image ▸ Mode ▸ 16 Bits/Channel; Layer ▸ Mask All Objects; Select ▸ Focus Area, Subject, Sky; Filter ▸ Neural
  Filters, Adaptive Wide Angle, Camera Raw Filter, Lens Correction, Blur Gallery (and its five entries), Video
  (and De-Interlace); View ▸ Proof Setup (the submenu itself).
- With the profile and proof tools refused, the Duotone arms behind them (`DuotoneDocument.exactProofImage`,
  Gray's profile route on a Duotone document, admission cases `assign-duotone16`/`convert-duotone16`) are
  unreachable from the app and MCP: the close removes them or records why they stay.

## The mode's scope (phase 2b's measurements are in the decisions above)

- Reached from an 8- or 16-bit Grayscale document only (Image ▸ Mode ▸ Duotone); Grayscale back drops the inks
  (phase 2b measured that Photoshop's conversion renders them into the gray, see What remains for the close).
- Type: Monotone, Duotone, Tritone, Quadtone.
- Each ink is a colour, a name and a Duotone Curve of 13 fields.
- Overprint Colors changes how overprinted inks display only, not the output.
- One channel, edited as Grayscale: edits, adjustments and filters behave as in Grayscale.
- Files: `.rz`, PSD and PSB keep the inks; every other export renders RGB through the view.
  EPS and DCS 2.0 are not planned.
- `.ado` presets load and save; Adobe's Pantone libraries are not shipped here.

## Open items

None outside "What remains for the close" above.

## Shipped runs

None yet.
